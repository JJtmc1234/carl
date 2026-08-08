//! `carl`, the command line front end.
//!
//! One command does the real work: `ask`. It records what you said, resumes the right
//! conversation, records what Carl said, and never deletes either.
//!
//! Slack is not wired up yet. When it is, it calls the same `respond` path with a thread id
//! built from the channel and thread timestamp, so the transport is the only new part.

mod ear;
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
            let answer = turn::respond(&home, &thread, &said, None)?;
            println!("{}", answer.text);
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

        Command::Listen { thread } => ear::Ear::new(ThreadId::new(thread)?)?.run(&home),

        Command::MicCheck { secs } => {
            use carl::audio::{Mic, SPEECH_FLOOR};
            let mic = Mic::open(secs as f32 + 1.0, std::path::Path::new("/dev/shm/carl"))?;

            println!("say \"hey carl, what should I do now\" ... recording {secs}s");
            mic.wait(secs as f32);

            let level = mic.loudness();
            println!("  level      {level:.3}   (speech floor is {SPEECH_FLOOR})");
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
