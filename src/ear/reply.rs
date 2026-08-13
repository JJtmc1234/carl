//! Answering a question, and writing down what was worth keeping.

use std::path::Path;

use anyhow::Result;
use carl::audio::Mic;

use super::mouth::Barge;
use super::narrate::Narration;
use super::{Ear, Running};
use crate::turn;

impl Ear {
    /// Answers one question, looking at the screen only if the question needs it.
    pub(super) fn answer(
        &self,
        running: &mut Running,
        mic: &Mic,
        home: &Path,
        question: &str,
        hush: f32,
    ) -> Result<()> {
        // Speaks each sentence as Claude writes it. The narration has to outlive the call so
        // the last sentence can finish playing, because dropping it kills the player mid word.
        let mut narration = Narration::new(
            &self.mouth,
            mic,
            hush,
            running.filler.as_mut().map(|f| f.arm()),
        );

        // Watched while nothing is arriving, which is when changing your mind actually
        // happens. Interrupting him mid sentence already worked, and interrupting him mid
        // thought did not, so the question you abandoned still got answered at you.
        let mut interrupt = Barge::new(hush);
        let mut waiting = || {
            if interrupt.saw(mic.recent(0.3)) {
                carl::Flow::Stop
            } else {
                carl::Flow::Continue
            }
        };

        let answer = {
            let mut on_text = |t: &str| narration.feed(t);
            if carl::needs_screen(question) {
                turn::look_in(
                    &mut running.pool,
                    home,
                    &self.thread,
                    question,
                    carl::Area::Screen,
                    &mut on_text,
                    &mut waiting,
                )
            } else {
                turn::stream_in(
                    &mut running.pool,
                    home,
                    &self.thread,
                    question,
                    None,
                    &mut on_text,
                    &mut waiting,
                )
            }
        };
        narration.finish();

        match answer {
            Ok(a) => {
                println!("{}", a.text);
                if a.interrupted {
                    eprintln!("  (interrupted)");
                }
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

    /// Gives Carl one last chance to write something down before the thread is dropped.
    ///
    /// It does not write anything itself. There is one way to keep a note and this is not a
    /// second one. An earlier version asked for a summary and saved the whole reply under a
    /// timestamped name, which could never replace itself, so the same fact restated at the
    /// end of ten conversations became ten notes that each cost context forever.
    ///
    /// This is still worth doing rather than dropping. The end of a conversation is the one
    /// moment when what mattered about it is obvious, and mid conversation Carl is answering
    /// a question rather than looking back at one.
    pub(super) fn remember(&self, running: &mut Running, home: &Path) -> Result<()> {
        let asked = "This conversation is ending. Look back over it and keep anything \
             genuinely worth carrying into future conversations: preferences, decisions, \
             facts about me. Use your [remember] lines, one fact each. Skip anything you \
             would not want repeated back weeks from now, and skip anything already kept. \
             If there is nothing worth keeping, reply with exactly NOTHING and no lines.";

        // The reply itself is thrown away. Anything worth keeping left it as a note on the
        // way through, and the closing summary is not something anybody wants read aloud.
        // No extra brief. Exchange already adds the identity, which is where the [remember]
        // instruction lives, and sending it twice would just take up room saying it again.
        if let Err(e) = turn::stream_in(
            &mut running.pool,
            home,
            &self.thread,
            asked,
            None,
            &mut |_| carl::Flow::Continue,
            &mut || carl::Flow::Continue,
        ) {
            eprintln!("could not look back over the conversation: {e:#}");
        }
        Ok(())
    }
}
