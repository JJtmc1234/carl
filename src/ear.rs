//! The loop. Listen, wake, answer, end, back to listening.
//!
//! Two states and four transitions, which is small enough to hold in your head, and that
//! matters because this is the code deciding what gets kept.
//!
//! ```text
//!   idle ──── nothing said to Carl ────> idle, and the audio is gone
//!   idle ──── "hey carl ..." ──────────> awake
//!  awake ──── anything else ───────────> answer it, stay awake
//!  awake ──── "end conversation" ──────> write a memory, back to idle
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use carl::audio::{Mic, SPEECH_FLOOR};
use carl::{Heard, ThreadId, Tier, Voice, Whisper, heard};

use crate::turn;

/// How much of the recent past Carl can see while idle. Comfortably longer than a wake word.
const WINDOW_SECS: f32 = 3.0;
/// How often the idle loop wakes up to look at that window.
const STEP_SECS: f32 = 0.6;
/// How long you have to stop talking before Carl decides you are done.
const HUSH_SECS: f32 = 0.9;
/// A hard stop, so a noisy room cannot record forever.
const CAP_SECS: f32 = 30.0;

pub struct Ear {
    whisper: Whisper,
    voice: Voice,
    thread: ThreadId,
}

impl Ear {
    pub fn new(thread: ThreadId) -> Result<Self> {
        Ok(Self {
            whisper: Whisper::found().context("whisper is not ready")?,
            voice: Voice::found().context("piper is not ready")?,
            thread,
        })
    }

    /// Runs until interrupted.
    pub fn run(&self, home: &Path) -> Result<()> {
        // Audio scratch lives in RAM. Nothing recorded ever reaches the disk, whether it is
        // kept or not, so the discard below is a real one.
        let mut mic = Mic::open(WINDOW_SECS, Path::new("/dev/shm/carl"))?;
        let mut awake = false;

        eprintln!("listening. say \"hey carl\" to start, \"end conversation\" to finish.");

        loop {
            if !awake {
                mic.advance(STEP_SECS)?;

                // Silence is most of a room's day. Running whisper over it costs real time
                // for a guaranteed empty answer, so the level check comes first.
                if mic.loudness() < SPEECH_FLOOR {
                    continue;
                }

                let text = self.whisper.transcribe(Tier::Wake, mic.snapshot()?)?;
                match heard::interpret(&text, false) {
                    // Not for Carl. Nothing is transcribed further, nothing is written down,
                    // and the window rolls the audio out of memory on its own.
                    Heard::Nothing => continue,
                    Heard::Wake { question } => {
                        awake = true;
                        mic.forget();
                        match question {
                            // Asked on the same breath, so answer it rather than making them
                            // say it twice.
                            Some(q) => self.answer(home, &q)?,
                            None => self.voice.say("Yes?")?,
                        }
                    }
                    // interpret only returns these when already listening.
                    Heard::Say(_) | Heard::End => continue,
                }
                continue;
            }

            // Awake. Capture a whole sentence rather than a window, because a sentence can
            // easily run longer than one.
            let wav = mic.utterance(HUSH_SECS, CAP_SECS)?;
            let text = self.whisper.transcribe(Tier::Talk, wav)?;

            match heard::interpret(&text, true) {
                Heard::End => {
                    self.remember(home)?;
                    self.voice.say("Alright. I'll remember that.")?;
                    awake = false;
                    mic.forget();
                    eprintln!("back to listening.");
                }
                Heard::Say(q) => self.answer(home, &q)?,
                // Nothing usually means the utterance was noise, not speech. Staying awake
                // rather than dropping out avoids the loop where a cough ends the
                // conversation and you have to wake him again.
                Heard::Nothing => continue,
                Heard::Wake { question } => {
                    if let Some(q) = question {
                        self.answer(home, &q)?;
                    }
                }
            }
        }
    }

    /// Answers one question, looking at the screen only if the question needs it.
    fn answer(&self, home: &Path, question: &str) -> Result<()> {
        let answer = if heard::needs_screen(question) {
            turn::look(home, &self.thread, question, carl::Area::Screen)
        } else {
            turn::respond(home, &self.thread, question, None)
        };

        match answer {
            Ok(a) => {
                println!("{}", a.text);
                self.voice.say(&a.text)?;
            }
            // Spoken, not just printed. You are looking at a game, not at this terminal, and
            // silence after a question is indistinguishable from Carl ignoring you.
            Err(e) => {
                eprintln!("failed: {e:#}");
                self.voice
                    .say("Sorry, something went wrong. Say that again?")?;
            }
        }
        Ok(())
    }

    /// Writes down what was worth keeping, then the thread can be forgotten.
    ///
    /// This is what "end conversation" buys. The full text stays in the record either way,
    /// but the record is not read back into future conversations. Only memory is.
    fn remember(&self, home: &Path) -> Result<()> {
        let asked = "Our conversation is ending. In three lines or fewer, write only what is \
             genuinely worth carrying into future conversations: preferences, decisions, \
             facts about me. Skip anything you would not want repeated back weeks from now. \
             If there is nothing worth keeping, reply with exactly NOTHING.";

        let note = match turn::respond(home, &self.thread, asked, None) {
            Ok(a) => a.text.trim().to_string(),
            Err(e) => {
                eprintln!("could not write a memory: {e:#}");
                return Ok(());
            }
        };

        if note.is_empty() || note.to_uppercase().contains("NOTHING") {
            return Ok(());
        }

        // Named by when it happened, so notes accumulate in order and never overwrite one
        // another.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default();

        let memory = carl::Memory::open(home.join("memory"))?;
        let path = memory.write(&format!("chat-{stamp}"), &note)?;
        eprintln!("remembered: {}", path.display());
        Ok(())
    }
}
