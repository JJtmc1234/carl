//! The accept loop, and one thread per connected panel.
//!
//! Blocking threads rather than an async runtime, because that is what the rest of Carl is and
//! because there is one panel. An executor would be more machinery than the thing it runs.
//!
//! **Liveness is a tail of the journal, not a rebuild.** The chain writes to `events.jsonl` from
//! its own process, so this one watches the file grow and forwards whatever is new. Rebuilding
//! the world on a timer would be simpler to write and would be wrong twice over: it would miss
//! anything that happened and was undone between two ticks, and it would give the panel no way
//! to tell one change from a redraw.
//!
//! The tail is a poll rather than inotify, which would need a dependency for the one thing it
//! would buy. The cost is up to `TICK` of latency on a file that is written a handful of times
//! per task. Written down because it is a choice and there is a point at which it stops being
//! the right one.

use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::command::{self, PanelCommand};
use super::facts::Facts;
use super::listen::Bound;
use super::permission;
use super::snapshot;
use super::waiting::Waiting;
use super::wire::{Ask, Frame, PanelEvent, Reply, Request, VERSION};
use crate::army::event::{self, Intervention, Journal, Record};
use crate::army::personnel::Personnel;
use crate::army::task::TaskId;
use crate::panel::view::PanelSnapshot;
use crate::providers::Diagnostics;
use crate::providers::projects::Projects;
use crate::{Result, ThreadId};

/// How often the journal is checked for new lines while a panel is subscribed.
const TICK: Duration = Duration::from_millis(150);

/// The conversation the panel talks to Carl in.
///
/// A thread of its own, like Slack's channels and the terminal's `cli`, so the panel has its own
/// history. It is the same Carl underneath with the same memory and the same rules, because it
/// goes through the same `turn` machinery every other surface goes through. A second Carl would
/// have needed a second code path, and there is not one.
pub const THREAD: &str = "panel";

pub struct Server {
    home: PathBuf,
    /// One sampler for the whole process, shared by every connection.
    ///
    /// Shared because the rate limit lives inside it. A `Diagnostics` per request would resample
    /// the machine on every snapshot, which is exactly the render rate polling that must not
    /// happen, and each one would start with an empty cache so the limit would never bite.
    machine: Arc<Mutex<Diagnostics>>,
    /// How long a question is held open before it refuses itself.
    ///
    /// A field rather than the constant read directly, so a test can prove the timeout refuses
    /// rather than grants without sitting there for the real ninety seconds.
    patience: Duration,
    /// Questions asked by a hook and not yet answered, shared by every connection.
    ///
    /// It has to be shared: the hook asks on one connection and JJ answers on another, so a
    /// per connection store would mean the answer never reached the process holding still.
    waiting: Arc<Waiting>,
}

impl Server {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            machine: Arc::new(Mutex::new(Diagnostics::new(&home))),
            waiting: Arc::new(Waiting::new()),
            patience: permission::WAIT,
            home,
        }
    }

    /// Shortens how long a question is held open. For tests.
    pub fn patience(mut self, how_long: Duration) -> Self {
        self.patience = how_long;
        self
    }

    /// Serves until the socket is dropped, which removes it. One thread per connection.
    ///
    /// Takes the `Bound` rather than a bare listener so the socket file cannot outlive the server
    /// that made it. Returning from here, by any route including a panic unwinding, unlinks it.
    pub fn run(&self, bound: Bound) -> Result<()> {
        for stream in bound.listener().incoming() {
            let stream = match stream {
                Ok(s) => s,
                // One panel failing to connect is not a reason to stop serving the next.
                Err(_) => continue,
            };
            let home = self.home.clone();
            let machine = Arc::clone(&self.machine);
            let waiting = Arc::clone(&self.waiting);
            let patience = self.patience;
            std::thread::spawn(move || {
                let _ = talk(&home, &machine, &waiting, patience, stream);
            });
        }
        Ok(())
    }
}

/// One connected panel, until it goes away.
pub fn talk(
    home: &Path,
    machine: &Mutex<Diagnostics>,
    waiting: &Waiting,
    patience: Duration,
    stream: UnixStream,
) -> Result<()> {
    let mut out = stream.try_clone()?;
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        // A line that will not parse is answered rather than dropped. A panel that sent
        // something malformed and got silence cannot tell that from a backend that died.
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                send(
                    &mut out,
                    &Frame::refused(None, format!("unreadable request: {e}")),
                )?;
                continue;
            }
        };

        if request.v != VERSION {
            send(
                &mut out,
                &Frame::refused(
                    Some(request.id),
                    format!(
                        "this backend speaks panel protocol {VERSION} and that frame said {}",
                        request.v
                    ),
                ),
            )?;
            continue;
        }

        let id = Some(request.id.clone());
        match request.body {
            Ask::Ping => send(&mut out, &Frame::to(id, Reply::Pong))?,
            Ask::Snapshot => match everything(home, machine) {
                Ok(s) => send(
                    &mut out,
                    &Frame::to(
                        id,
                        Reply::Snapshot {
                            snapshot: Box::new(s),
                        },
                    ),
                )?,
                Err(e) => send(&mut out, &Frame::refused(id, e.to_string()))?,
            },
            // Takes over the connection. A subscribed panel is streaming, and interleaving
            // request handling on the same socket would need framing this protocol does not
            // have. A panel that wants both opens two connections.
            Ask::Subscribe { since } => {
                return stream_from(home, machine, waiting, &mut out, id, since);
            }

            // Takes over the connection. The hook has nothing else to say and is holding a tool
            // call still, so it parks here until JJ answers or the wait runs out.
            Ask::MayI { request } => {
                let asked = request.clone();
                let answer = match waiting.ask(request) {
                    Some(rx) => rx,
                    None => {
                        send(
                            &mut out,
                            &Frame::to(
                                id,
                                Reply::Settled {
                                    question: asked.id,
                                    verdict: permission::Verdict::Deny,
                                },
                            ),
                        )?;
                        continue;
                    }
                };
                let verdict = answer
                    .recv_timeout(patience)
                    .unwrap_or(permission::Verdict::Deny);
                waiting.give_up(&asked.id);
                send(
                    &mut out,
                    &Frame::to(
                        id,
                        Reply::Settled {
                            question: asked.id,
                            verdict,
                        },
                    ),
                )?;
            }

            Ask::Answered {
                question: which,
                verdict,
            } => {
                let landed = waiting.answer(&which, verdict);
                eprintln!(
                    "[permission] answer for {which} arrived: {} ({})",
                    verdict.word(),
                    if landed {
                        "landed"
                    } else {
                        "nothing was waiting"
                    }
                );
                send(
                    &mut out,
                    &Frame::to(
                        id,
                        Reply::Done {
                            seq: None,
                            what: if landed {
                                format!("{which} answered {}", verdict.word())
                            } else {
                                format!("nothing was waiting on {which}, it has already gone")
                            },
                        },
                    ),
                )?;
            }
            Ask::Command { command } => {
                let replies = carry_out(home, command, &mut out, id.clone());
                match replies {
                    Ok(reply) => send(&mut out, &Frame::to(id, reply))?,
                    Err(e) => send(&mut out, &Frame::refused(id, e.to_string()))?,
                }
            }
        }
    }
    Ok(())
}

/// The whole world, army and providers together.
///
/// The task fold happens once here and is handed to the providers, because the project join
/// needs it and the snapshot needs it, and folding twice would be the same work for the same
/// answer.
fn everything(home: &Path, machine: &Mutex<Diagnostics>) -> Result<PanelSnapshot> {
    let people = Personnel::open(home)?;
    let records = event::read(people.journal_path())?;
    let tasks = super::tasks::fold(&records);

    let facts = {
        // Held only while sampling. A slow read of /proc must not stop another panel taking a
        // snapshot of the army, which costs nothing and is what it usually wants.
        let mut machine = machine.lock().map_err(|_| {
            crate::Error::Refused("the diagnostics sampler panicked in another thread".into())
        })?;
        Facts::gather(&mut machine, &Projects::open(home), &tasks).with_runtime(home)
    };
    snapshot::build_from(&people, &records, &facts)
}

fn send(out: &mut UnixStream, frame: &Frame) -> Result<()> {
    writeln!(out, "{}", serde_json::to_string(frame)?)?;
    out.flush()?;
    Ok(())
}

/// Replays what the panel missed, then forwards everything new until it disconnects.
fn stream_from(
    home: &Path,
    machine: &Mutex<Diagnostics>,
    waiting: &Waiting,
    out: &mut UnixStream,
    id: Option<String>,
    since: u64,
) -> Result<()> {
    let path = Personnel::open(home)?.journal_path();
    let records = event::read(&path)?;

    if let Some(gap) = gap(&records, since) {
        return send(out, &Frame::to(id, gap));
    }

    let mut last = since;
    for r in records.iter().filter(|r| r.seq > since) {
        last = r.seq;
        send(out, &event_frame(r.clone()))?;
    }
    // Sent even when the replay was empty, so a panel always knows it has caught up rather than
    // having to infer it from a quiet connection.
    send(out, &Frame::to(id, Reply::Live { seq: last }))?;

    // Where the sampler had got to when this panel subscribed. Telemetry is only sent when this
    // moves, so a subscriber gets a frame when there is genuinely something newer and not on a
    // timer of its own.
    let mut told_of = machine.lock().ok().and_then(|m| m.last_sampled_at());

    // Everything already waiting, because a panel opened after the question was asked is exactly
    // the panel that needs to see it. Outcomes start from now: a question this panel never saw,
    // already settled, is not news.
    let mut outcomes_upto = waiting.settled_now();
    let mut shown: HashSet<String> = HashSet::new();
    for request in waiting.outstanding() {
        shown.insert(request.id.clone());
        eprintln!(
            "[permission] offering {} ({}) to a subscriber",
            request.id, request.tool
        );
        if send(out, &Frame::to(None, Reply::Permission { request })).is_err() {
            return Ok(());
        }
    }

    loop {
        let fresh: Vec<Record> = event::read(&path)?
            .into_iter()
            .filter(|r| r.seq > last)
            .collect();
        for r in fresh {
            last = r.seq;
            // A write failure is the panel having gone away, which is ordinary.
            if send(out, &event_frame(r)).is_err() {
                return Ok(());
            }
        }

        if let Some(frame) = fresh_telemetry(machine, &mut told_of)
            && send(out, &frame).is_err()
        {
            return Ok(());
        }

        // Questions, and questions that have stopped being questions. Polled on the same tick as
        // the journal because a panel is a screen and a tick is faster than a person.
        for request in waiting.outstanding() {
            if !shown.insert(request.id.clone()) {
                continue;
            }
            eprintln!(
                "[permission] pushing {} ({}) live",
                request.id, request.tool
            );
            if send(out, &Frame::to(None, Reply::Permission { request })).is_err() {
                return Ok(());
            }
        }
        let (settled, upto) = waiting.settled_after(outcomes_upto);
        outcomes_upto = upto;
        for outcome in settled {
            shown.remove(&outcome.id);
            eprintln!(
                "[permission] telling a subscriber {} is settled as {}",
                outcome.id,
                outcome.verdict.word()
            );
            let frame = Frame::to(
                None,
                Reply::Settled {
                    question: outcome.id,
                    verdict: outcome.verdict,
                },
            );
            if send(out, &frame).is_err() {
                return Ok(());
            }
        }
        std::thread::sleep(TICK);
    }
}

/// A telemetry frame, but only if the sampler has actually taken a new reading.
///
/// Asking the sampler on every tick is free: its own rate limit decides whether to resample, and
/// between samples it hands back the previous readings with their original `measured_at`. So the
/// interval that governs how often a panel hears about the machine is Process 3's, and there is
/// no second one here that could drift from it.
///
/// The comparison is on `last_sampled_at` rather than on the readings, because two identical
/// readings taken a minute apart are two different facts and a panel showing an age needs to
/// know the second one happened.
fn fresh_telemetry(machine: &Mutex<Diagnostics>, told_of: &mut Option<u64>) -> Option<Frame> {
    let mut machine = machine.lock().ok()?;
    let sampled = machine.machine();
    let at = machine.last_sampled_at()?;

    if *told_of == Some(at) {
        return None;
    }
    *told_of = Some(at);
    Some(Frame::to(
        None,
        Reply::Telemetry {
            at,
            diagnostics: sampled,
        },
    ))
}

fn event_frame(r: Record) -> Frame {
    Frame::to(
        None,
        Reply::Event {
            event: Box::new(PanelEvent::of(r)),
        },
    )
}

/// Whether the record can honour a request to continue from `since`.
///
/// Two ways it cannot. The panel asks for a sequence past the end, which means it is holding a
/// position from a different record than this one, most likely because the journal was replaced.
/// Or the record no longer starts early enough to reach `since + 1`. Both are answered honestly
/// rather than served as a stream with a hole in it that would look continuous.
fn gap(records: &[Record], since: u64) -> Option<Reply> {
    let first = records.first().map_or(0, |r| r.seq);
    let last = records.last().map_or(0, |r| r.seq);

    if since > last {
        return Some(Reply::Gap {
            asked_for: since,
            have_from: first,
            have_to: last,
            why: "that sequence is past the end of this record, so it is not the record you were \
                  reading. Take a fresh snapshot."
                .into(),
        });
    }
    if since > 0 && first > since + 1 {
        return Some(Reply::Gap {
            asked_for: since,
            have_from: first,
            have_to: last,
            why: "the record no longer goes back that far. Take a fresh snapshot.".into(),
        });
    }
    None
}

/// Does what the panel asked, and writes down anything that reached past the chain.
fn carry_out(
    home: &Path,
    command: PanelCommand,
    out: &mut UnixStream,
    id: Option<String>,
) -> Result<Reply> {
    command.check()?;

    // The journal is opened only where something is written, because opening it creates the
    // directory it lives in. `say` and `inspect` change nothing, and a command that changes
    // nothing must leave nothing behind: a `run/` directory that appeared because somebody
    // looked would be indistinguishable from one where an army had worked and stopped.
    let intervention = match &command {
        PanelCommand::Say { text } => return speak(home, text, out, id),
        PanelCommand::Objective { text } => {
            // An objective goes to Carl as well as into the record, because an objective nobody
            // was told about is a note to self.
            let mut journal = open_journal(home)?;
            let recorded =
                command::record(&mut journal, Intervention::Objective { what: text.clone() })?;

            // And then it is handed to a lead, which is the step that used to be missing. Being
            // told and answering in conversation left every objective sitting in the record with
            // nothing done about it, so the work came back to whoever was at the keyboard.
            //
            // Which lead is Carl's judgement and is asked for. Whether he may have that lead is
            // not, and is checked against the organisation before anything is written.
            let handed = hand_objective_down(home, &mut journal, recorded.seq, text, out);

            // And straight on down, because a lead holding work is the state this whole thing
            // exists to end. JJ chose "on objective" over a timer: work flows the moment he asks
            // for something, and an army nobody has asked anything of costs nothing.
            let onward = match &handed {
                Ok((lead, _)) => carry_on_down(home, lead, out),
                Err(_) => None,
            };

            let what = match &handed {
                Ok((lead, task)) => format!(
                    "objective recorded at {}, handed to {lead} as task {task}, {} notified{}",
                    recorded.seq,
                    recorded.told.len(),
                    match &onward {
                        Some(note) => format!(". {note}"),
                        None => String::new(),
                    }
                ),
                // Recorded and told, but not moved. Said plainly rather than swallowed: an
                // objective JJ believes is being worked on and is not is the worst outcome here.
                Err(why) => format!(
                    "objective recorded at {} and Carl told, but it was not handed down. {why}",
                    recorded.seq
                ),
            };
            return Ok(Reply::Done {
                seq: Some(recorded.seq),
                what,
            });
        }
        PanelCommand::Inspect { agent } => {
            let people = Personnel::open(home)?;
            let records = event::read(people.journal_path())?;
            let view = snapshot::inspect(&people, &records, agent)?;
            return Ok(Reply::Done {
                seq: None,
                what: serde_json::to_string(&view)?,
            });
        }
        PanelCommand::Answer { seq, text } => Intervention::Answered {
            question: seq.to_string(),
            answer: text.clone(),
        },
        // Recorded, and then actually delivered. Until now this wrote the intervention down and
        // stopped, so JJ could message an agent from the panel and never hear back: the record
        // said he had spoken to them and nobody had. Hunter asked to be able to talk to Miles
        // from the panel, and a message nobody answers is not talking to them.
        PanelCommand::JjMessage { agent, text } => {
            let mut journal = open_journal(home)?;
            let recorded = command::record(
                &mut journal,
                Intervention::Message {
                    to: agent.clone(),
                    what: text.clone(),
                },
            )?;
            let answer = ask_agent(home, agent, text, out)
                .unwrap_or_else(|why| format!("{agent} did not answer. {why}"));
            return Ok(Reply::Done {
                seq: Some(recorded.seq),
                what: answer,
            });
        }
        PanelCommand::JjInstruct { agent, instruction } => Intervention::Override {
            agent: agent.clone(),
            instruction: instruction.clone(),
        },
        PanelCommand::JjStop { agent, why } => Intervention::Stopped {
            task: holding(home, agent)?,
            why: why.clone(),
        },
        PanelCommand::JjReplace { agent, goal, why } => Intervention::Replaced {
            task: holding(home, agent)?,
            goal: goal.clone(),
            why: why.clone(),
        },
    };

    let mut journal = open_journal(home)?;
    let recorded = command::record(&mut journal, intervention)?;
    Ok(Reply::Done {
        seq: Some(recorded.seq),
        what: format!(
            "recorded as a JJ intervention, told {}",
            recorded.told.join(" and ")
        ),
    })
}

/// Opens the record for writing, which creates its directory.
///
/// Separate so that every caller of it is visibly a writer. Reads go through `event::read`,
/// which returns nothing for a file that is not there rather than making one.
fn open_journal(home: &Path) -> Result<Journal> {
    Journal::open(Personnel::open(home)?.journal_path())
}

/// The task this agent is actually holding, refusing rather than guessing.
///
/// Stopping "whatever she is doing" when she is doing nothing has to be an error. Inventing a
/// task id would put a stop event in the record against a task that never existed.
fn holding(home: &Path, agent: &str) -> Result<TaskId> {
    let people = Personnel::open(home)?;
    people
        .state(agent)
        .and_then(|s| s.holding.clone())
        .ok_or_else(|| {
            crate::Error::Refused(format!(
                "{agent} is not holding a task, so there is none to stop"
            ))
        })
}

/// Asks Carl, through the same machinery every other surface uses.
///
/// Text is forwarded as it arrives rather than at the end, because `turn::stream` already hands
/// it over that way and holding it back would make the panel slower than the terminal for no
/// reason.
/// Asks Carl which lead an objective belongs to, and hands it to them.
///
/// The answer is streamed to the panel as he writes it, so JJ watches the decision being made
/// rather than waiting on a silent socket and then being told what happened.
///
/// Returns the lead and the task id, or why it did not move. Every failure is a refusal to
/// write rather than a guess: a wrong owner chosen quietly is worse than an objective that
/// visibly did not move, because nobody goes looking for the second one.
fn hand_objective_down(
    home: &Path,
    journal: &mut Journal,
    objective_seq: u64,
    text: &str,
    out: &mut UnixStream,
) -> std::result::Result<(String, String), String> {
    let asked = crate::army::chain::objective::ask_which_lead(text);
    let thread = ThreadId::new(THREAD).map_err(|e| e.to_string())?;

    let answer = crate::turn::stream(home, &thread, &asked, None, None, &mut |chunk| {
        match send(out, &Frame::to(None, frame_for(chunk))) {
            Ok(()) => crate::claude::Flow::Continue,
            Err(_) => crate::claude::Flow::Stop,
        }
    })
    .map_err(|e| e.to_string())?;

    let chosen = crate::army::chain::objective::read_choice(&answer.text);
    let (_record, task) = crate::army::chain::objective::hand_down(journal, objective_seq, &chosen)
        .map_err(|e| e.to_string())?;

    Ok((chosen.lead, task.id.to_string()))
}

/// Asks a lead to hand on whatever it is now holding, one step.
///
/// Called straight after Carl delegates, so an objective reaches somebody who will actually do
/// it rather than stopping at the lead. Returns a sentence for JJ, or `None` when there was
/// nothing to move.
///
/// Failure here is deliberately not failure of the objective. The objective is recorded and the
/// lead has it either way, and a lead that could not be reached is a thing to say rather than a
/// reason to pretend nothing happened.
fn carry_on_down(home: &Path, lead: &str, out: &mut UnixStream) -> Option<String> {
    let mut board = crate::army::board::Board::open(home).ok()?;
    let stuck = crate::army::chain::work::waiting_on_a_lead(&board).ok()?;
    let task = stuck.into_iter().find(|t| t.owner == lead)?;

    let asked = crate::army::chain::assign::ask_which_agent(lead, &task.goal).ok()?;
    let said = ask_agent(home, lead, &asked, out).ok()?;

    let mut people = Personnel::open(home).ok();
    match crate::army::chain::work::hand_on_one(&mut board, people.as_mut(), lead, &task, &said) {
        Ok((agent, _)) => Some(format!("{lead} handed it to {agent}")),
        Err(why) => Some(format!("{lead} could not hand it on. {why}")),
    }
}

/// Puts a message to one named agent and hands back what they said.
///
/// Runs the agent in its own process with its own brief, rather than asking Carl to relay. Carl
/// hands work to leads and does not see inside their departments, so a relayed answer would be
/// Carl guessing on somebody else's behalf.
///
/// Tools come from the agent's rank, and from a small extra list where the job needs one: Miles
/// reads mail, so he is given the Gmail calls that read and draft and none that send or delete.
/// That boundary is the permission list rather than a sentence in his brief.
fn ask_agent(
    home: &Path,
    agent: &str,
    said: &str,
    out: &mut UnixStream,
) -> std::result::Result<String, String> {
    let who = crate::army::org::require(agent).map_err(|e| e.to_string())?;
    let people = Personnel::open(home).map_err(|e| e.to_string())?;

    // The agent's own memory is what makes this the same agent as last time.
    let mut brief = crate::army::chain::brief_for(who);
    let summary = people.folder(agent).join("memory").join("summary.md");
    if let Ok(extra) = std::fs::read_to_string(&summary) {
        brief.push_str("\n\nWhat you keep between conversations:\n");
        brief.push_str(&extra);
    }
    let rules = people.folder(agent).join("memory").join("rules.md");
    if let Ok(extra) = std::fs::read_to_string(&rules) {
        brief.push_str("\n\n");
        brief.push_str(&extra);
    }

    let mut tools = crate::army::chain::tools_for(who.rank);
    tools.extend(extra_tools_for(agent).into_iter().map(str::to_owned));

    let _ = send(
        out,
        &Frame::to(
            None,
            Reply::Speaking {
                text: format!("[{agent}]\n"),
            },
        ),
    );

    // The message goes in on stdin, not as a trailing argument.
    //
    // `--allowedTools` takes a list, so anything after it on the command line is read as another
    // tool name. With the tools last, the message was swallowed and `claude` exited saying no
    // input was given at all, which reached JJ as "miles did not answer". Ordering the flags to
    // avoid it works but is one edit away from breaking again. Stdin cannot be eaten by a flag.
    // Streamed rather than waited for. The blocking form sent one "thinking..." and then
    // nothing at all until the whole answer landed, so an agent reading forty files and an
    // agent that had wedged looked exactly alike for as long as it took.
    let mut cmd = std::process::Command::new("claude");
    cmd.arg("-p")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("--append-system-prompt")
        .arg(&brief)
        .arg("--permission-mode")
        .arg("acceptEdits");
    if !tools.is_empty() {
        cmd.arg("--allowedTools").arg(tools.join(","));
    }
    cmd.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| "claude gave us no stdin to write to".to_string())?;
        pipe.write_all(said.as_bytes()).map_err(|e| e.to_string())?;
    }
    // Read here rather than on a thread. Nothing else is waiting on this connection, and the
    // child keeps writing into the pipe while a frame is being sent.
    let mut said = String::new();
    if let Some(pipe) = child.stdout.take() {
        use std::io::BufRead;
        for line in std::io::BufReader::new(pipe)
            .lines()
            .map_while(std::result::Result::ok)
        {
            let Some(chunk) = crate::claude::chunk_of(&line) else {
                continue;
            };
            let frame = match chunk {
                crate::claude::Chunk::Text(t) => {
                    said.push_str(&t);
                    Reply::Speaking { text: t }
                }
                crate::claude::Chunk::Thinking(t) => Reply::Thinking { text: t },
                crate::claude::Chunk::Doing { tool, detail } => Reply::Doing { tool, detail },
                crate::claude::Chunk::Refused { tool, why } => Reply::Speaking {
                    text: crate::claude::refusal_line(&tool, &why),
                },
                // The envelope repeats the whole answer. Kept only as the fallback for a
                // stream that produced no text deltas at all, never appended to what arrived.
                crate::claude::Chunk::Final(a) => {
                    if said.trim().is_empty() {
                        said = a.text.clone();
                        Reply::Speaking { text: a.text }
                    } else {
                        continue;
                    }
                }
            };
            if send(out, &Frame::to(None, frame)).is_err() {
                // The panel hung up. Nothing is reading the rest of this.
                break;
            }
        }
    }
    let done = child.wait_with_output().map_err(|e| e.to_string())?;
    let said = said.trim().to_string();
    if said.is_empty() {
        return Err(String::from_utf8_lossy(&done.stderr).trim().to_string());
    }
    Ok(said)
}

/// Calls one agent needs that its rank does not imply.
///
/// Read and draft only for Miles. No send, no trash, no spam marking: an agent that cannot call
/// them cannot be talked into calling them, which is a stronger promise than telling him not to.
fn extra_tools_for(agent: &str) -> Vec<&'static str> {
    match agent {
        "miles" => vec![
            "mcp__claude_ai_Gmail__search_threads",
            "mcp__claude_ai_Gmail__get_thread",
            "mcp__claude_ai_Gmail__get_message",
            "mcp__claude_ai_Gmail__list_drafts",
            "mcp__claude_ai_Gmail__create_draft",
            "mcp__claude_ai_Gmail__update_draft",
        ],
        _ => Vec::new(),
    }
}

/// Turns one piece of a turn into the frame that carries it.
///
/// One place, so the panel's two streaming paths cannot drift into showing different things.
/// A refusal keeps its prose because it is addressed to a person and says what to do about it.
fn frame_for(say: crate::claude::Say<'_>) -> Reply {
    match say {
        crate::claude::Say::Words(t) => Reply::Speaking { text: t.into() },
        crate::claude::Say::Thinking(t) => Reply::Thinking { text: t.into() },
        crate::claude::Say::Doing { tool, detail } => Reply::Doing {
            tool: tool.into(),
            detail: detail.into(),
        },
        crate::claude::Say::Refused { tool, why } => Reply::Speaking {
            text: crate::claude::refusal_line(tool, why),
        },
    }
}

fn speak(home: &Path, said: &str, out: &mut UnixStream, id: Option<String>) -> Result<Reply> {
    let thread = ThreadId::new(THREAD)?;
    let answer = crate::turn::stream(home, &thread, said, None, None, &mut |chunk| {
        match send(out, &Frame::to(None, frame_for(chunk))) {
            Ok(()) => crate::claude::Flow::Continue,
            // The panel hung up mid answer. Stopping is right: the rest is going nowhere.
            Err(_) => crate::claude::Flow::Stop,
        }
    })
    .map_err(|e| crate::Error::Claude(e.to_string()))?;

    let _ = id;
    Ok(Reply::Done {
        seq: None,
        what: answer.text,
    })
}
