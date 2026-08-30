//! `carl`, the command line front end.
//!
//! One command does the real work: `ask`. It records what you said, resumes the right
//! conversation, records what Carl said, and never deletes either.
//!
//! Slack is not wired up yet. When it is, it calls the same `respond` path with a thread id
//! built from the channel and thread timestamp, so the transport is the only new part.

mod chat;
mod ear;
mod repl;
mod supervise;

use std::path::PathBuf;

use anyhow::Result;
use carl::{Memory, ThreadId, turn};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "carl", version, about = "A helper that remembers")]
struct Cli {
    /// Where the record, the thread registry and the memory notes live.
    #[arg(long, default_value = "~/.carl", global = true)]
    home: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Say something to Carl and get an answer.
    Ask {
        message: Vec<String>,
        /// Which conversation. The same thread continues where it left off.
        #[arg(long, default_value = "cli")]
        thread: String,
    },

    /// Take a picture of the screen and ask about it.
    ///
    /// Works for any game or any window, not just one. The screenshot goes into Carl's
    /// workspace and is replaced each time rather than piling up.
    Look {
        question: Vec<String>,
        #[arg(long, default_value = "cli")]
        thread: String,
        /// Just the focused window instead of the whole screen.
        #[arg(long)]
        window: bool,
    },

    /// Listen on the microphone until interrupted.
    ///
    /// Say "hey carl" to start, "end conversation" to finish. Anything not addressed to
    /// Carl is never transcribed past the wake check and never written down.
    Listen {
        #[arg(long, default_value = "voice")]
        thread: String,

        /// Seconds of quiet before Carl decides you have finished talking.
        ///
        /// Lower feels snappier and starts cutting you off when you pause to think. There is
        /// no right answer here, only your right answer, so it is a flag rather than a guess.
        #[arg(long, default_value_t = 0.4)]
        hush: f32,

        /// Seconds before Carl gives up waiting for you to stop.
        #[arg(long, default_value_t = 15.0)]
        cap: f32,

        /// Use the larger transcription model. Costs about two seconds on every turn.
        ///
        /// Worth it in a noisy room or when item names keep coming out wrong. On clean audio
        /// the smaller model gave the identical transcript three times faster.
        #[arg(long)]
        accurate: bool,
    },

    /// Talk to Carl by typing, keeping one conversation open.
    ///
    /// Unlike `ask`, this holds the process between questions, so the second question costs
    /// nothing to set up. Control d or "exit" to finish.
    Chat {
        #[arg(long, default_value = "cli")]
        thread: String,
    },

    /// Answer in Slack until interrupted.
    ///
    /// Mention @Carl in a channel he has been invited to, or send him a direct message.
    /// Needs ~/.carl/slack.json with a bot token and an app token. See readme.md.
    Slack,

    /// Say something in a Slack channel without being asked.
    ///
    /// Carl has to be in the channel already. Invite him with /invite @Carl.
    Say {
        /// A channel id like C01ABC, or a name like #general.
        channel: String,
        message: Vec<String>,
    },

    /// Open an A2A exchange with another agent in a channel.
    ///
    /// The protocol is in docs/a2a.md. With no message this sends a hello, which is how you
    /// find out whether the other agent speaks it.
    Greet {
        /// A channel id like C01ABC, or a name like #general.
        channel: String,
        /// The other agent's Slack user id, the U... in their mention.
        agent: String,
        message: Vec<String>,
    },

    /// Check the microphone and report what each stage actually heard.
    ///
    /// Run this when Carl is not responding. It shows the level, what the cheap wake model
    /// heard, what the better model heard, and whether that would have woken him, so the
    /// failing stage names itself instead of having to be guessed at.
    MicCheck {
        /// Seconds to record.
        #[arg(long, default_value_t = 5)]
        secs: u64,
    },

    /// Show a conversation as it was recorded.
    History {
        #[arg(long, default_value = "cli")]
        thread: String,
        /// Show every conversation instead of one.
        #[arg(long)]
        all: bool,
    },

    /// List the conversations Carl is holding.
    Threads,

    /// Send one request down the chain of command and wait for it to come back.
    ///
    /// JJ to Carl to Adrian to Mason to Nora, and all the way back up. Four real `claude`
    /// processes, one per agent, each with the tools its rank allows and no others.
    Chain {
        request: Vec<String>,
        /// Where Nora does the work. She can change files here and nowhere else matters.
        #[arg(long)]
        workdir: String,
        /// Where to write what happened. Defaults to events.jsonl inside the workdir.
        #[arg(long)]
        journal: Option<String>,
    },

    /// Hand one piece of work to one of your direct reports, and wait for the answer.
    ///
    /// The real route down the chain, and the only one. Refuses anything that is not an edge in
    /// the organisation and says which lead to go through instead.
    ///
    /// This exists because refusing the fake route was not enough on its own. Carl had no way to
    /// reach Olivia, so given the built in subagent tool he spawned a process and told it "you
    /// are Miles", and once that was taken away he did the work himself. Both are the chief
    /// doing a department's job.
    Handoff {
        /// Who is handing the work over. Must be able to delegate to `to`.
        #[arg(long)]
        from: String,
        /// Which of their direct reports is being given it.
        #[arg(long)]
        to: String,
        /// How long to let them work, in seconds.
        #[arg(long)]
        deadline: Option<u64>,
        /// The work itself, as an outcome rather than an implementation.
        work: Vec<String>,
    },

    /// Decide one tool call by asking the panel. Run by Claude Code, not by a person.
    ///
    /// Reads a PreToolUse payload on stdin and prints the decision. Denies whenever it cannot
    /// get an answer, including when no panel is running, and always exits zero, because a hook
    /// that exits non zero is ignored rather than obeyed.
    PermitHook {
        /// Who is asking, as the panel should show it. `jj` or an agent's name.
        ///
        /// Passed rather than worked out from the working directory. Two agents can share a
        /// directory, and a guess would put one agent's name on another's question.
        #[arg(long = "as", default_value = "jj")]
        surface: String,

        /// Print the settings that install this hook, instead of deciding a call.
        ///
        /// For putting the hook somewhere other than a Carl run, and for checking what is
        /// actually being installed rather than guessing from the source.
        #[arg(long)]
        settings: bool,
    },

    /// The army itself, before anything is running.
    Army {
        #[command(subcommand)]
        action: ArmyAction,
    },
    /// Keep every agent's process running, and nothing else.
    ///
    /// The supervisor owns process existence. It starts an agent that is not running, resumes
    /// the conversation of one whose process was replaced, backs off one that keeps falling
    /// over, and gives up on one that will not start at all. It hands out no work: that is
    /// Carl's, and there is deliberately no way to do it from here.
    ///
    /// One supervisor per home. A second is refused rather than allowed to fight the first
    /// over every agent.
    Supervise {
        /// Run one pass and stop, which is what a check is.
        #[arg(long)]
        once: bool,
        /// Seconds between passes.
        #[arg(long, default_value_t = 5)]
        every: u64,
        /// Which `claude` to run, for pointing at a different build.
        #[arg(long, default_value = "claude")]
        claude: String,
    },
    /// Serve the Command Panel backend on a local socket.
    ///
    /// Owner only, under ~/.carl/panel/. Nothing listens on the network. The protocol is in
    /// docs/panel-v1.md, and it is line delimited JSON, so `nc -U` is a working client when the
    /// thing that is broken is the backend.
    Panel,
    /// What Carl remembers across conversations.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
    },
}

#[derive(Subcommand)]
enum ArmyAction {
    /// Give every agent in the organisation a folder, an id and a memory folder.
    ///
    /// JJ's act, and recorded as his, because there is no chief with a folder yet who could
    /// have authorised it. Refuses a home that already holds an army rather than merging into
    /// one, so running it twice by accident cannot half rewrite what is there.
    Found,
    /// Who has a folder, and what each of them is holding.
    Who,

    /// Every agent at once, as the chart, with anything wrong called out.
    ///
    /// `who` answers what each agent is holding. This answers whether each agent can work at
    /// all, which is a different question: a process can be up, a session can be resumed, and
    /// the folder the agent keeps everything in can be gone.
    Status {
        /// One line of counts, for something reading rather than somebody looking.
        ///
        /// A stable contract. The tree above is laid out for a person and is free to change
        /// with taste, and anything parsing it would break the next time it did.
        #[arg(long)]
        brief: bool,
    },

    /// Everything known about one agent, from every source that knows something.
    Inspect {
        /// The agent's name in the organisation.
        agent: String,
    },

    /// Recent things that actually happened, folded from the journal.
    ///
    /// The journal is append only and is already the durable history, so this reads it rather
    /// than keeping a second copy that could disagree with it.
    Activity {
        /// Only what this agent did, and what was done to it. Everybody, when left out.
        agent: Option<String>,
        /// How many to show. The newest ones.
        #[arg(long, default_value_t = 20)]
        last: usize,
    },

    /// Put an agent the supervisor gave up on back in the ordinary queue.
    ///
    /// Giving up is deliberate: six starts that did not stick means starting it again is not the
    /// fix. `wake` refuses a degraded agent for that reason and the refusal is right. What was
    /// missing is the way out. A transient fault took all ten agents down on 2026 08 28 and the
    /// army stayed down for twenty one hours because no command existed to say the cause had
    /// been looked at.
    ///
    /// This clears the verdict and nothing else. It starts no process and promises nothing: the
    /// supervisor's ordinary policy decides what happens on its next pass.
    Revive {
        /// Which agents. All of the given up ones, when left out.
        agents: Vec<String>,
        /// Also abandon the recorded conversation so the next start begins a new one.
        ///
        /// For when the recorded session names a conversation that no longer exists. Every
        /// resume then fails the same way forever and reviving alone just repeats the loop.
        /// The agent keeps its memory folder, which is what continuity actually rests on.
        #[arg(long)]
        fresh: bool,
    },

    /// Bring existing memory folders up to the current layout, without touching what is in them.
    ///
    /// `found` refuses a home that already holds an army and there is deliberately no other way
    /// to re-save everybody, so a folder made before a layout change would otherwise never get
    /// the new files. Every step asks whether the thing is already there, so running this twice
    /// is indistinguishable from running it once.
    Migrate,

    /// Give a folder to somebody in the organisation who does not have one yet.
    ///
    /// What `found` cannot do, because `found` is for an empty home and refuses one that
    /// already holds an army. This is how agents added to `army::org` join a home that is
    /// already running, which is what happened when the organisation grew from four to ten.
    ///
    /// Recorded as Carl's act, because who exists and where they sit is the chief's, subject to
    /// JJ. It invents nobody: an agent has to be in the table already, and adding a row there is
    /// a change to compiled code on purpose.
    Enlist {
        /// Which agents. With none, everybody in the table who has no folder.
        who: Vec<String>,
    },
    /// Move whatever the chain is sitting on, one step.
    ///
    /// A lead holding work that it has not handed on is a stalled chain, and this is what
    /// unsticks it: the lead is asked which of its own people should do the thing, and the
    /// answer is checked against the organisation before anything is written.
    ///
    /// One step per task on purpose. Driving a whole campaign in one call cannot be
    /// interrupted, cannot be watched, and spends money in a shape nobody chose.
    Work {
        /// Say what would move without asking anybody or writing anything.
        #[arg(long)]
        dry_run: bool,
    },

    /// Whether the army is getting better, folded out of the record.
    ///
    /// The nine measures in `docs/flagship-workflow.md`. Nothing is recorded for them
    /// separately, so there is no second file that could come to disagree with the history.
    Metrics {
        /// How many of the most recent objectives the trend is taken over.
        #[arg(long, default_value_t = 10)]
        recent: usize,
    },
}

#[derive(Subcommand)]
enum MemoryAction {
    /// Show everything Carl would be told at the start of a conversation.
    Show,
    /// Write or replace one note.
    Write { name: String, body: Vec<String> },
    /// Delete one note.
    Forget { name: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let home = expand(&cli.home);

    match cli.command {
        Command::Ask { message, thread } => {
            let thread = ThreadId::new(thread)?;
            let said = message.join(" ");
            if said.trim().is_empty() {
                anyhow::bail!("nothing to say");
            }
            // Streamed rather than waited for, so the terminal shows the answer being
            // written. Same path the voice uses, which means running this exercises it.
            use std::io::Write;
            let mut out = std::io::stdout();
            // Accumulated and stripped rather than printed straight through. The raw stream
            // carries Carl's [remember] and [forget] lines, which are bookkeeping and not
            // part of the answer, and printing them and then not having them in the final
            // text is the worst of both. Slack already did this and the terminal did not.
            let mut seen = String::new();
            let mut printed = 0usize;

            // No voice brief here. This is being read, not heard, and one or two sentences is
            // the wrong shape for a terminal.
            let answer = turn::stream(&home, &thread, &said, None, None, &mut |t| {
                // Notes go straight out, dimmed, and are never accumulated. Only the words are
                // the answer, so only the words are stripped, printed and remembered.
                let t = match t {
                    carl::Say::Words(t) => t,
                    carl::Say::Thinking { text, tokens } => {
                        // The text is usually redacted and the size is what there is, so a
                        // silent terminal would be the only sign of a long think.
                        match (text.is_empty(), tokens) {
                            (true, Some(n)) => {
                                let _ = write!(out, "\x1b[2m[thinking, ~{n} tokens]\x1b[0m");
                            }
                            (true, None) => {}
                            (false, _) => {
                                let _ = write!(out, "\x1b[2m{text}\x1b[0m");
                            }
                        }
                        let _ = out.flush();
                        return carl::Flow::Continue;
                    }
                    carl::Say::Doing { tool, detail } => {
                        let _ = write!(
                            out,
                            "\x1b[2m{}\x1b[0m",
                            carl::claude::doing_line(tool, detail)
                        );
                        let _ = out.flush();
                        return carl::Flow::Continue;
                    }
                    carl::Say::Refused { tool, why } => {
                        let _ = write!(out, "{}", carl::claude::refusal_line(tool, why));
                        let _ = out.flush();
                        return carl::Flow::Continue;
                    }
                };
                seen.push_str(t);
                let visible = carl::remember::split(&seen).text;
                // Only ever appends, and only on a character boundary. Stripping a note can
                // shorten the visible text and nothing already printed can be taken back, and
                // `printed` is a byte offset into an older version of the string, so slicing
                // at it blindly could cut a character in half and panic.
                if let Some(fresh) = visible.get(printed..)
                    && !fresh.is_empty()
                {
                    let _ = out.write_all(fresh.as_bytes());
                    let _ = out.flush();
                    printed = visible.len();
                }
                carl::Flow::Continue
            })?;
            println!();
            if answer.text.trim().is_empty() {
                anyhow::bail!("claude returned an empty answer");
            }
            Ok(())
        }

        Command::Look {
            question,
            thread,
            window,
        } => {
            let thread = ThreadId::new(thread)?;
            let asked = question.join(" ");
            if asked.trim().is_empty() {
                anyhow::bail!("ask something about the screen");
            }
            let area = if window {
                carl::Area::Window
            } else {
                carl::Area::Screen
            };
            let answer = turn::look(&home, &thread, &asked, area)?;
            println!("{}", answer.text);
            Ok(())
        }

        Command::Listen {
            thread,
            hush,
            cap,
            accurate,
        } => ear::Ear::new(ThreadId::new(thread)?, accurate)?.run(&home, ear::Timing { hush, cap }),

        Command::Chat { thread } => repl::run(&home, &thread),

        Command::Slack => chat::run(&home),

        Command::Say { channel, message } => {
            let text = message.join(" ");
            if text.trim().is_empty() {
                anyhow::bail!("nothing to say");
            }
            chat::say(&home, &channel, &text)
        }

        Command::Greet {
            channel,
            agent,
            message,
        } => chat::greet(&home, &channel, &agent, &message.join(" ")),

        Command::MicCheck { secs } => {
            use carl::audio::{Mic, SPEECH_FLOOR};
            let devices = carl::aec::Devices::detect();
            let mic = Mic::open(
                secs as f32 + 1.0,
                std::path::Path::new("/dev/shm/carl"),
                devices.source(),
            )?;
            match devices.source() {
                Some(d) => println!("  device     {d} (echo cancelled, so you can interrupt him)"),
                None => println!(
                    "  device     default. No echo canceller, so Carl goes deaf while \
                     speaking.\n             Start one with: pipewire -c etc/carl-aec.conf"
                ),
            }

            // Measure the empty room first. This is the number that decides when Carl
            // thinks you have stopped talking, and getting it wrong the loud way makes him
            // record until his cap on every turn.
            print!("hold still, measuring the room... ");
            use std::io::Write;
            std::io::stdout().flush().ok();
            let hush = mic.calibrate(1.5);
            let room = mic.loudness();
            println!("room {room:.3}, so quiet means below {hush:.3}");

            println!("say \"hey carl, what should I do now\" ... recording {secs}s");
            mic.forget();
            mic.wait(secs as f32);

            let level = mic.loudness();
            println!("  level      {level:.3}   (rms while speaking)");
            if level < hush {
                println!("  -> that was no louder than the room. Nothing to transcribe.");
                return Ok(());
            }
            if level < SPEECH_FLOOR {
                println!("  -> too quiet. Raise the mic in Settings, Sound, Input.");
                return Ok(());
            }

            let wav = mic.snapshot()?;
            let w = carl::Whisper::found()?;
            let wake = w.transcribe(carl::Tier::Wake, wav)?;
            let talk = w.transcribe(carl::Tier::Talk, wav)?;
            println!("  wake model heard   {wake:?}");
            println!("  talk model heard   {talk:?}");

            match carl::heard::interpret(&wake, false) {
                carl::Heard::Wake { question } => {
                    println!("  -> would have woken. Question: {question:?}")
                }
                _ => println!(
                    "  -> would NOT have woken. Say \"hey carl\" clearly, or add a spelling \
                     to NAMES in heard.rs if the model wrote something odd above."
                ),
            }
            Ok(())
        }

        Command::History { thread, all } => {
            let entries = carl::log::read(home.join("conversations.jsonl"))?;
            let shown = if all {
                entries
            } else {
                carl::log::thread(&entries, &ThreadId::new(thread)?)
            };

            if shown.is_empty() {
                println!("nothing recorded yet");
                return Ok(());
            }
            for e in shown {
                let who = match e.speaker {
                    carl::Speaker::Human => e.author.unwrap_or_else(|| "you".into()),
                    carl::Speaker::Carl => "carl".into(),
                    carl::Speaker::System => "system".into(),
                };
                println!("[{}] {who}: {}", e.thread, e.text);
            }
            Ok(())
        }

        Command::Army { action } => match action {
            ArmyAction::Found => {
                let army = carl::army::personnel::found(&home, now())?;
                for name in army.names() {
                    let id = army
                        .identity(name)
                        .map_or_else(|| "no id".to_string(), |i| i.id.to_string());
                    println!("  {name:8} {id}");
                }
                println!("{} agents enlisted in {}", army.len(), home.display());
                Ok(())
            }
            ArmyAction::Status { brief } => {
                let all = carl::army::survey::everyone(&home)?;
                match brief {
                    true => println!("{}", carl::army::survey::brief_of(&all)),
                    false => print_status(&all),
                }
                Ok(())
            }

            ArmyAction::Inspect { agent } => {
                print_inspect(&carl::army::survey::one(&home, &agent)?);
                Ok(())
            }

            ArmyAction::Activity { agent, last } => {
                let recent = carl::army::survey::activity(&home, agent.as_deref(), last)?;
                if recent.is_empty() {
                    match &agent {
                        Some(who) => println!("nothing recorded about {who}"),
                        None => println!("nothing has been recorded in {}", home.display()),
                    }
                    return Ok(());
                }
                for record in &recent {
                    println!("{}", carl::army::survey::line_of(record));
                }
                Ok(())
            }

            ArmyAction::Revive { agents, fresh } => {
                use carl::army::runtime::revive::{self, Revived};

                // Named agents, or everybody the survey says was given up on. Deriving the
                // second from the survey rather than from a second scan means the list is
                // exactly what `carl army status` just showed.
                let wanted: Vec<String> = match agents.is_empty() {
                    false => agents,
                    true => carl::army::survey::everyone(&home)?
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.runtime.as_ref().map(|r| &r.lifecycle),
                                Some(carl::army::runtime::Lifecycle::Degraded { .. })
                            )
                        })
                        .map(|s| s.agent.name.to_string())
                        .collect(),
                };

                if wanted.is_empty() {
                    println!("nobody has been given up on.");
                    return Ok(());
                }

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let mut cleared = 0usize;
                for name in &wanted {
                    match revive::one(&home, name, fresh, now)? {
                        Revived::Cleared { was } => {
                            cleared += 1;
                            println!("  {name:8} back in the queue. It was given up on: {was}");
                        }
                        Revived::SessionAbandoned => println!(
                            "  {name:8} was already in the queue. Its recorded conversation \
                             is abandoned, so the next start begins a new one"
                        ),
                        Revived::NotGivenUp(why) => println!("  {name:8} left alone, {why}"),
                        Revived::NoRecord => {
                            println!("  {name:8} has no runtime record, so nothing to undo")
                        }
                    }
                }
                if cleared > 0 {
                    println!(
                        "\n{cleared} cleared. The supervisor decides on its next pass, and it \
                         will give up again if the cause is still there."
                    );
                }
                Ok(())
            }

            ArmyAction::Migrate => {
                let people = carl::army::personnel::Personnel::open(&home)?;
                let mut touched = 0usize;
                for name in people.names() {
                    let folder = people.folder(name);
                    carl::army::personnel::memory::migrate(&folder)?;
                    touched += 1;
                }
                println!("checked {touched} memory folders, added only what was missing");
                Ok(())
            }
            ArmyAction::Who => {
                let army = carl::army::personnel::Personnel::open(&home)?;
                if army.is_empty() {
                    println!("no army has been founded in {}", home.display());
                    return Ok(());
                }
                for name in army.names() {
                    let holding = army
                        .state(name)
                        .and_then(|s| s.holding.as_ref())
                        .map_or_else(|| "idle".to_string(), |t| format!("holding {t}"));
                    println!("  {name:8} {holding}");
                }
                for missing in army.missing() {
                    println!("  {:8} no folder yet", missing.name);
                }
                Ok(())
            }

            ArmyAction::Enlist { who } => {
                use carl::army::personnel::{enlist, founding_config, founding_profile};

                let mut army = carl::army::personnel::Personnel::open(&home)?;
                let missing: Vec<String> =
                    army.missing().iter().map(|a| a.name.to_string()).collect();

                // Checked against the table before anything is written, so a typo names the
                // agents that exist rather than half enlisting the ones spelled right.
                let wanted: Vec<String> = if who.is_empty() {
                    missing
                } else {
                    for name in &who {
                        carl::army::org::require(name)?;
                    }
                    who
                };
                if wanted.is_empty() {
                    // Nothing to enlist, but the generated files can still be describing an
                    // older organisation, so this is the pass that makes them agree with it.
                    for agent in carl::army::org::everyone() {
                        if agent.rank != carl::army::org::Rank::Human {
                            army.write_readme(agent.name)?;
                        }
                    }
                    println!("everybody in the organisation already has a folder");
                    println!("READMEs rewritten from the table, nothing else touched");
                    return Ok(());
                }

                let mut journal = carl::army::event::Journal::open(army.journal_path())?;
                for name in wanted {
                    if army.get(&name).is_some() {
                        // The folder stays exactly as it is and no event is written, so running
                        // this again is free. The README is the one exception: it is generated
                        // from the table and never read back, so rewriting it is how a folder
                        // founded under an older organisation stops describing a reporting line
                        // that no longer exists.
                        army.write_readme(&name)?;
                        println!("  {name:8} already had one, README refreshed");
                        continue;
                    }
                    // Hours come from the rank rather than the default, so somebody enlisted
                    // into a running home gets the ordinary overnight window instead of
                    // running all night because nobody said otherwise.
                    let rank = carl::army::org::require(&name)?.rank;
                    let logged = enlist(
                        &mut army,
                        &mut journal,
                        "carl",
                        &name,
                        founding_profile(&name),
                        founding_config(rank),
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0),
                    )?;
                    println!("  {name:8} enlisted, recorded at seq {}", logged.seq);
                }
                Ok(())
            }

            ArmyAction::Metrics { recent } => {
                let path = home.join("run").join("events.jsonl");
                if !path.exists() {
                    println!("nothing has been recorded in {}", home.display());
                    return Ok(());
                }
                let m = carl::army::metrics::of(&carl::army::event::read(&path)?);
                print_metrics(&m, recent);
                Ok(())
            }
            ArmyAction::Work { dry_run } => {
                use carl::army::board::Board;
                use carl::army::chain::work;

                let board = Board::open(&home)?;
                let stuck = work::waiting_on_a_lead(&board)?;
                let to_review = work::waiting_on_review(&board)?;

                if stuck.is_empty() && to_review.is_empty() {
                    println!(
                        "nothing to do. Nobody is sitting on work and nothing is waiting on a review."
                    );
                    return Ok(());
                }
                for task in &stuck {
                    println!("  down: {:8} holds {}  {}", task.owner, task.id, task.goal);
                }
                for task in &to_review {
                    println!(
                        "  up:   {:8} submitted {}, {} reviews",
                        task.owner, task.id, task.created_by
                    );
                }
                if dry_run {
                    println!("\nnothing was asked and nothing was written.");
                    return Ok(());
                }

                // One at a time, and the leads are asked in the order the board holds them, so
                // a run that is interrupted has done a whole number of handovers rather than
                // half of one.
                let mut board = Board::open(&home)?;
                // The folders too, so an agent given work reads as busy on screen rather than
                // holding a task in the record and idle in the panel.
                let mut people = carl::army::personnel::Personnel::open(&home)?;

                // Up before down. Accepting finished work frees the agent who did it, so doing
                // this first means the pass can hand them something else rather than finding
                // them still busy with a task that was done before it started.
                for task in to_review {
                    let asked =
                        carl::army::chain::words::review_asked(&task.goal, task.id.as_str());
                    let said = match ask_one_agent(&home, &task.created_by, &asked) {
                        Ok(text) => text,
                        Err(e) => {
                            println!("  {:8} did not review: {e}", task.created_by);
                            continue;
                        }
                    };
                    match work::review_one(&mut board, Some(&mut people), &task, &said) {
                        Ok((true, why)) => {
                            println!("  {:8} accepted {}. {why}", task.created_by, task.id)
                        }
                        Ok((false, why)) => {
                            println!("  {:8} sent {} back. {why}", task.created_by, task.id)
                        }
                        Err(e) => println!("  {:8} {e}", task.created_by),
                    }
                }
                for task in stuck {
                    let asked =
                        match carl::army::chain::assign::ask_which_agent(&task.owner, &task.goal) {
                            Ok(q) => q,
                            Err(e) => {
                                println!("  {:8} {e}", task.owner);
                                continue;
                            }
                        };
                    let said = match ask_one_agent(&home, &task.owner, &asked) {
                        Ok(text) => text,
                        Err(e) => {
                            println!("  {:8} did not answer: {e}", task.owner);
                            continue;
                        }
                    };
                    match work::hand_on_one(
                        &mut board,
                        Some(&mut people),
                        &task.owner,
                        &task,
                        &said,
                    ) {
                        Ok((agent, made)) => {
                            println!("  {:8} -> {:8} {}", task.owner, agent, made.goal)
                        }
                        // Said out loud rather than swallowed. A lead that keeps naming
                        // somebody else's agent is a thing JJ needs to know about.
                        Err(e) => println!("  {:8} {e}", task.owner),
                    }
                }
                Ok(())
            }
        },

        Command::Supervise {
            once,
            every,
            claude,
        } => supervise::run(&home, &claude, every, once),

        Command::Panel => {
            let at = carl::panel::socket_path(&home);
            let held = carl::panel::listen::hold(&at)?;
            // systemd stops a service with SIGTERM, so without this every ordinary stop would
            // leave a socket behind and `ls` would suggest a backend was running.
            carl::panel::listen::on_signal(&at)?;
            println!("panel backend listening on {}", at.display());
            carl::panel::Server::new(&home).run(held)?;
            Ok(())
        }
        Command::Threads => {
            let registry = carl::Registry::open(home.join("threads.json"))?;
            let entries = carl::log::read(home.join("conversations.jsonl"))?;

            if registry.is_empty() {
                println!("no conversations yet");
                return Ok(());
            }
            // Counted from the record rather than tracked separately, so the two cannot
            // drift apart and disagree.
            let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
            for e in &entries {
                *counts.entry(e.thread.to_string()).or_default() += 1;
            }
            for (thread, n) in counts {
                println!("{thread}  {n} message(s)");
            }
            Ok(())
        }

        Command::Handoff {
            from,
            to,
            deadline,
            work,
        } => {
            let work = work.join(" ");
            let deadline = deadline
                .map(std::time::Duration::from_secs)
                .unwrap_or(carl::army::chain::DEADLINE);
            let handed = carl::army::handoff::hand(
                &home,
                std::path::Path::new("claude"),
                &from,
                &to,
                &work,
                deadline,
            )?;
            // The answer on its own, so whoever handed the work over reads what came back and
            // not a report about it having come back.
            println!("{}", handed.said);
            if let Some(seq) = handed.seq {
                eprintln!("recorded at {seq}");
            }
            Ok(())
        }

        Command::Chain {
            request,
            workdir,
            journal,
        } => {
            let asked = request.join(" ");
            if asked.trim().is_empty() {
                anyhow::bail!("ask for something");
            }
            let workdir = expand(&workdir);
            let journal = journal
                .map(|j| expand(&j))
                .unwrap_or_else(|| workdir.join("events.jsonl"));

            println!("JJ asks: {asked}\n");
            let mut chain = carl::army::Chain::new("claude", &workdir, &journal)?.aloud(true);
            let passage = carl::army::campaign(&mut chain, &asked)?;

            println!("\n--- what each agent was handed ---");
            println!("\nadrian to mason:\n{}", passage.for_mason);
            println!("\nmason to nora:\n{}", passage.for_nora);

            println!("\n--- the tasks ---");
            for t in &passage.tasks {
                println!(
                    "  {} {:<7} from {:<7} {} after {} attempt(s)",
                    t.id, t.owner, t.created_by, t.status, t.attempts
                );
            }

            println!("\n--- carl to JJ ---\n{}", passage.answer);
            println!("\nthe record is at {}", journal.display());
            if !passage.accepted {
                anyhow::bail!(
                    "the work was not accepted after {} attempts",
                    passage.attempts
                );
            }
            Ok(())
        }

        Command::PermitHook { surface, settings } => {
            if settings {
                let me = std::env::current_exe()?;
                println!("{}", carl::claude::asking::settings(&me, &home, &surface));
                return Ok(());
            }
            print!(
                "{}",
                carl::panel::hook::run(&home, &surface, &mut std::io::stdin())
            );
            Ok(())
        }

        Command::Memory { action } => {
            let memory = Memory::open(home.join("memory"))?;
            match action {
                MemoryAction::Show => match memory.assemble()? {
                    Some(text) => println!("{text}"),
                    None => println!("Carl remembers nothing yet"),
                },
                MemoryAction::Write { name, body } => {
                    let path = memory.write(&name, &body.join(" "))?;
                    println!("wrote {}", path.display());
                }
                MemoryAction::Forget { name } => {
                    if memory.forget(&name)? {
                        println!("forgot {name}");
                    } else {
                        println!("there was no memory called {name}");
                    }
                }
            }
            Ok(())
        }
    }
}

/// Unix seconds, for the commands that write a time down.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Expands a leading `~`, because a default of `~/.carl` is otherwise taken literally and
/// creates a directory actually named `~`.
fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

/// The nine measures, printed so a gap reads as a gap.
///
/// A rate over nothing is left blank rather than shown as zero or as a hundred percent. Both
/// would be a score, and an army that has never been asked to do anything has not earned one.
/// The army as the chart, with anything wrong called out beside the agent it is wrong for.
///
/// A tree because the reporting line is the thing an operator navigates by. A flat list sorted
/// by name puts Miles between Mason and Nora, where nobody is looking for him.
fn print_status(all: &[carl::army::survey::Standing]) {
    use carl::army::survey::Standing;

    if all.iter().all(|s| !s.enlisted) {
        println!("no army has been founded here. `carl army found` gives everybody a folder.");
        return;
    }

    fn row(s: &Standing, depth: usize) {
        let indent = "  ".repeat(depth + 1);
        let state = match (&s.enlisted, &s.runtime) {
            (false, _) => "not enlisted".to_string(),
            (true, None) => "no process record".to_string(),
            (true, Some(r)) => carl::army::survey::lifecycle_word(&r.lifecycle).to_string(),
        };
        let holding = s.holding.as_deref().unwrap_or("idle");
        let name = format!("{indent}{}", s.agent.name);
        println!("{name:<20} {state:<18} {holding}");
        if let Some(worry) = s.worry() {
            println!("{:<20} ! {worry}", format!("{indent}"));
        }
    }

    fn under(all: &[Standing], boss: Option<&str>, depth: usize) {
        for s in all.iter().filter(|s| s.agent.reports_to == boss) {
            row(s, depth);
            under(all, Some(s.agent.name), depth + 1);
        }
    }

    println!("jj");
    under(all, Some("jj"), 0);

    let worried: Vec<&Standing> = all.iter().filter(|s| s.worry().is_some()).collect();
    match worried.len() {
        0 => println!("\nnothing needs attention."),
        n => println!("\n{n} of {} need attention, marked with !", all.len()),
    }
}

/// Everything known about one agent, from every source that knows something.
fn print_inspect(s: &carl::army::survey::Standing) {
    let a = s.agent;
    println!("{} ({})", a.display, a.name);
    println!("  rank        {:?}", a.rank);
    println!("  reports to  {}", a.reports_to.unwrap_or("nobody"));
    println!(
        "  hands to    {}",
        match s.reports.is_empty() {
            true => "nobody, so this agent does the work it is given".to_string(),
            false => s
                .reports
                .iter()
                .map(|r| r.name)
                .collect::<Vec<_>>()
                .join(", "),
        }
    );
    println!("  remit       {}", a.remit);
    println!(
        "  model       {}",
        s.model.as_deref().unwrap_or("no folder, so unset")
    );
    println!(
        "  holding     {}",
        s.holding.as_deref().unwrap_or("nothing")
    );

    match &s.runtime {
        // Absent is not stopped. Nobody has said, and saying "stopped" would be inventing it.
        None => println!("  process     no supervisor has written about this agent"),
        Some(r) => {
            println!(
                "  process     {}",
                carl::army::survey::lifecycle_word(&r.lifecycle)
            );
            if let Some(session) = &r.session {
                println!("  session     {session}");
            }
            // Three separate answers, not one blob. "the process was replaced and the session
            // was resumed and the memory was kept" is the sentence somebody actually wants.
            if let Some(c) = &r.continuity {
                println!(
                    "  continuity  process {:?}, session {:?}, memory {:?}",
                    c.process, c.session, c.memory
                );
            }
        }
    }

    println!("  memory      {}", s.health.memory.word());
    println!("    summary   {} bytes", s.health.summary_bytes);
    match (s.health.rules, s.health.watching) {
        (Some(rules), Some(watching)) => {
            println!("    learned   {rules} rules, {watching} being watched")
        }
        _ => println!("    learned   unreadable"),
    }
    if s.health.legacy_rules {
        println!("    rules.md  present, superseded by learned.md");
    }

    if let Some(worry) = s.worry() {
        println!("\n  ! {worry}");
    }
}

fn print_metrics(m: &carl::army::Metrics, recent: usize) {
    fn rate(part: usize, whole: usize) -> String {
        match whole {
            0 => "  n/a".to_string(),
            _ => format!("{:5.0}%", 100.0 * part as f64 / whole as f64),
        }
    }

    let all = m.objectives.len();
    println!("objectives          {all}");
    println!(
        "  accepted          {}  {}",
        m.accepted(),
        rate(m.accepted(), all)
    );
    println!(
        "  without JJ        {}  {}",
        m.unattended(),
        rate(m.unattended(), all)
    );
    match m.interventions_each() {
        Some(each) => println!("  interventions     {each:.2} each"),
        None => println!("  interventions     n/a, nothing has been asked for yet"),
    }

    // The trend, which is the only thing a single figure cannot show. Printed only once there
    // are two windows to compare, because the last ten against themselves says nothing.
    let latest = m.latest(recent);
    if all > latest.len() {
        let earlier = &m.objectives[..all - latest.len()];
        let per = |set: &[carl::army::metrics::Objective]| {
            set.iter().map(|o| o.interventions).sum::<usize>() as f64 / set.len() as f64
        };
        println!(
            "  trend             {:.2} each over the first {}, {:.2} over the last {}",
            per(earlier),
            earlier.len(),
            per(latest),
            latest.len()
        );
    }

    let reviews = m.reviews.accepted + m.reviews.rejected;
    println!("reviews             {reviews}");
    println!(
        "  rejected          {}  {}",
        m.reviews.rejected,
        rate(m.reviews.rejected, reviews)
    );
    println!("  escalations       {}", m.escalations);

    println!("submissions         {}", m.retries.submissions);
    println!(
        "  repeats           {}  {}",
        m.retries.repeats,
        rate(m.retries.repeats, m.retries.submissions)
    );

    println!("crashes             {}", m.recovery.crashes);
    println!(
        "  recovered         {}  {}",
        m.recovery.resumed,
        rate(m.recovery.resumed, m.recovery.crashes)
    );
    println!("  gave up           {}", m.recovery.gave_up);
    if m.recovery.outstanding > 0 {
        println!(
            "  unanswered        {}, which is somebody to go and look at",
            m.recovery.outstanding
        );
    }

    println!("continuity losses   {}", m.continuity_failures);
    println!("refusals            {}", m.refusals);
    if m.loose_interventions > 0 {
        println!(
            "loose interventions {}, naming no task",
            m.loose_interventions
        );
    }
}

/// Puts one question to one agent and hands back what it said.
///
/// Its own process with its own brief, rather than asking Carl to relay. Carl hands work to
/// leads and does not see inside their departments, so a relayed answer would be Carl guessing
/// on somebody else's behalf.
fn ask_one_agent(home: &std::path::Path, agent: &str, question: &str) -> Result<String> {
    let who = carl::army::org::require(agent)?;
    let people = carl::army::personnel::Personnel::open(home)?;

    let mut brief = carl::army::chain::brief_for(who);
    let summary = people.folder(agent).join("memory").join("summary.md");
    if let Ok(extra) = std::fs::read_to_string(&summary) {
        brief.push_str("\n\nWhat you keep between conversations:\n");
        brief.push_str(&extra);
    }

    let out = std::process::Command::new("claude")
        .arg("-p")
        .arg("--append-system-prompt")
        .arg(&brief)
        .arg(question)
        .output()?;

    let said = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if said.is_empty() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        anyhow::bail!(match why.is_empty() {
            true => "said nothing at all".to_string(),
            false => why,
        });
    }
    Ok(said)
}
