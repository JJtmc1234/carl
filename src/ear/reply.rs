//! Answering a question, and writing down what was worth keeping.

use std::path::Path;

use anyhow::Result;
use carl::audio::Mic;

use super::Ear;
use crate::turn;

impl Ear {
    /// Answers one question, looking at the screen only if the question needs it.
    pub(super) fn answer(&self, mic: &Mic, home: &Path, question: &str, hush: f32) -> Result<()> {
        let answer = if carl::needs_screen(question) {
            turn::look(home, &self.thread, question, carl::Area::Screen)
        } else {
            turn::respond(home, &self.thread, question, None)
        };

        match answer {
            Ok(a) => {
                println!("{}", a.text);
                self.mouth.say(mic, &a.text, hush)?;
            }
            // Spoken, not just printed. You are looking at a game, not at this terminal, and
            // silence after a question is indistinguishable from Carl ignoring you.
            Err(e) => {
                eprintln!("failed: {e:#}");
                self.mouth
                    .say(mic, "Sorry, something went wrong. Say that again?", hush)?;
            }
        }
        Ok(())
    }

    /// Writes down what was worth keeping, then the thread can be forgotten.
    ///
    /// This is what "end conversation" buys. The full text stays in the record either way,
    /// but the record is not read back into future conversations. Only memory is.
    pub(super) fn remember(&self, home: &Path) -> Result<()> {
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
