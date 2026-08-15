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
mod turn;

use std::path::PathBuf;

use anyhow::Result;
use carl::{Memory, ThreadId};
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

    /// What Carl remembers across conversations.
    Memory {
        #[command(subcommand)]
        action: MemoryAction,
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
            println!("\ncarl to adrian:\n{}", passage.for_adrian);
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
