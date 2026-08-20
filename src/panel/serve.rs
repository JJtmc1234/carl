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

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::command::{self, PanelCommand};
use super::facts::Facts;
use super::listen::Bound;
use super::snapshot;
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
}

impl Server {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            machine: Arc::new(Mutex::new(Diagnostics::new(&home))),
            home,
        }
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
            std::thread::spawn(move || {
                let _ = talk(&home, &machine, stream);
            });
        }
        Ok(())
    }
}

/// The two fields every frame carries, read before the body is decoded.
///
/// Its own permissive struct because a body the parser dislikes must not cost the id. Unknown
/// fields are ignored here on purpose: the point is to salvage the envelope from a frame whose
/// body will not parse. See bug 24.
#[derive(Debug, Default, serde::Deserialize)]
struct Envelope {
    #[serde(default)]
    v: Option<u32>,
    #[serde(default)]
    id: Option<String>,
}

/// One incoming line, or the refusal to send back instead.
///
/// Two steps rather than one, and the order matters both times.
///
/// The envelope comes first so a refusal can carry the id. `docs/panel-v1.md` tells panels that
/// a frame with no id was not asked for and must not be treated as an answer, so a refusal
/// without one is discarded by a panel following the documentation, which then waits for a
/// reply that never arrives. That is worse than an unhelpful error: it is a hang.
///
/// The version comes before the body, because a version this backend does not speak is the
/// reason the body will not parse, and reporting the symptom instead of the cause sends
/// somebody looking in the wrong place. A newer panel is told which protocol this speaks,
/// which is exactly what `docs/panel-v1.md` advertises as how a hopeful caller finds out.
fn decode(line: &str) -> std::result::Result<Request, Frame> {
    // A line that is not JSON at all has no envelope to salvage, and `None` for the id is then
    // the truth rather than a loss.
    let envelope: Envelope = serde_json::from_str(line).unwrap_or_default();
    let id = envelope.id.clone();

    if let Some(v) = envelope.v
        && v != VERSION
    {
        return Err(Frame::refused(
            id,
            format!("this backend speaks panel protocol {VERSION} and that frame said {v}"),
        ));
    }

    serde_json::from_str::<Request>(line)
        .map_err(|e| Frame::refused(id, format!("unreadable request: {e}")))
}

/// One connected panel, until it goes away.
pub fn talk(home: &Path, machine: &Mutex<Diagnostics>, stream: UnixStream) -> Result<()> {
    let mut out = stream.try_clone()?;
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        // A line that will not parse is answered rather than dropped. A panel that sent
        // something malformed and got silence cannot tell that from a backend that died.
        let request = match decode(&line) {
            Ok(r) => r,
            Err(refusal) => {
                send(&mut out, &refusal)?;
                continue;
            }
        };

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
            Ask::Subscribe { since } => return stream_from(home, machine, &mut out, id, since),
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
        Facts::gather(&mut machine, &Projects::open(home), &tasks)
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
            let _ = speak(home, &format!("New objective from JJ: {text}"), out, id);
            return Ok(Reply::Done {
                seq: Some(recorded.seq),
                what: format!(
                    "objective recorded and Carl told, {} notified",
                    recorded.told.len()
                ),
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
        PanelCommand::JjMessage { agent, text } => Intervention::Message {
            to: agent.clone(),
            what: text.clone(),
        },
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
fn speak(home: &Path, said: &str, out: &mut UnixStream, id: Option<String>) -> Result<Reply> {
    let thread = ThreadId::new(THREAD)?;
    let answer = crate::turn::stream(home, &thread, said, None, None, &mut |text| {
        match send(out, &Frame::to(None, Reply::Speaking { text: text.into() })) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(line: &str) -> Frame {
        match decode(line) {
            Err(f) => f,
            Ok(r) => panic!("expected a refusal, got {r:?}"),
        }
    }

    fn why(frame: &Frame) -> &str {
        match &frame.body {
            Reply::Refused { why } => why,
            other => panic!("expected a refusal, got {other:?}"),
        }
    }

    /// The bug. The whole `Request` was parsed before the version was looked at, so a body the
    /// parser disliked was refused with no id at all.
    ///
    /// `docs/panel-v1.md` tells panels a frame with no id was not asked for and must not be
    /// treated as an answer. So a panel following the documentation discards the refusal and
    /// waits for a reply that never comes. That is worse than an unhelpful error message, it is
    /// a hang.
    #[test]
    fn a_refusal_carries_the_id_the_frame_arrived_with() {
        for line in [
            r#"{"v":1,"id":"a","ask":"pause"}"#,
            r#"{"v":1,"id":"a","ask":"command","command":{"kind":"say","text":"hi","actor":"m"}}"#,
            r#"{"v":2,"id":"a","ask":"ping"}"#,
        ] {
            let frame = refusal(line);
            assert_eq!(
                frame.id.as_deref(),
                Some("a"),
                "the id was on the frame and has to come back: {line}"
            );
        }
    }

    /// A version this backend does not speak is the reason the body will not parse, so it is
    /// reported instead of the symptom. `docs/panel-v1.md` advertises this as how a newer panel
    /// finds out what is spoken here.
    #[test]
    fn a_version_mismatch_is_named_rather_than_reported_as_unreadable() {
        let frame = refusal(r#"{"v":2,"id":"a","ask":"pause"}"#);
        let text = why(&frame);
        assert!(text.contains("protocol 1"), "{text}");
        assert!(text.contains("said 2"), "{text}");
        assert!(
            !text.contains("unreadable"),
            "the version is the cause, not the symptom: {text}"
        );
    }

    /// A line that is not JSON at all has no envelope to salvage, so no id is the truth rather
    /// than a loss.
    #[test]
    fn a_line_that_is_not_json_refuses_without_an_id() {
        let frame = refusal("this is not json");
        assert_eq!(frame.id, None);
        assert!(why(&frame).contains("unreadable"));
    }

    /// An ordinary frame still decodes, which is the case that must not be broken by any of
    /// the above.
    #[test]
    fn a_good_frame_still_decodes() {
        let r = decode(r#"{"v":1,"id":"a","ask":"ping"}"#).expect("a ping is fine");
        assert_eq!(r.id, "a");
        assert_eq!(r.body, Ask::Ping);
    }
}
