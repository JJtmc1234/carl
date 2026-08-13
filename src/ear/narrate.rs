//! Speaking an answer while it is still being written.
//!
//! This is where the felt latency of the whole thing lives. Claude takes five to twenty five
//! seconds to finish an answer and under one second to start it. Waiting for the last word
//! before saying the first spends all of that on silence, and a machine that pauses twenty
//! seconds and then talks is not having a conversation with you.
//!
//! So each sentence is spoken as it lands, while Claude writes the next one.

use carl::Flow;
use carl::audio::Mic;
use carl::speech::{Sentences, Speaking};

use super::mouth::{Barge, Mouth, RECENT_SECS, Said, wait_out};

pub struct Narration<'a> {
    mouth: &'a Mouth,
    mic: &'a Mic,
    barge: Barge,
    /// The sentence currently coming out of the speakers, if any.
    speaking: Option<Speaking>,
    pending: Sentences,
    cut_off: bool,
}

impl<'a> Narration<'a> {
    pub fn new(mouth: &'a Mouth, mic: &'a Mic, hush: f32) -> Self {
        Self {
            mouth,
            mic,
            barge: Barge::new(hush),
            speaking: None,
            pending: Sentences::new(),
            cut_off: false,
        }
    }

    /// Takes the next piece of the answer.
    ///
    /// Blocks while a finished sentence is spoken, which is deliberate. A reader thread keeps
    /// draining Claude's output the whole time, so Claude carries on writing at full speed
    /// and the words queue up rather than being lost.
    pub fn feed(&mut self, text: &str) -> Flow {
        if self.cut_off {
            return Flow::Stop;
        }
        self.pending.feed(text);

        while let Some(sentence) = self.pending.take() {
            if self.speak(&sentence) == Said::CutOff {
                return Flow::Stop;
            }
        }
        Flow::Continue
    }

    /// Says the last of it and waits for the speakers to fall silent.
    ///
    /// Separate from `feed` because the tail of an answer usually has no full stop after it,
    /// and because dropping a `Narration` mid sentence would kill the player. The last words
    /// of every reply would be cut off, which is the kind of bug that sounds like a bad
    /// connection rather than a mistake in the code.
    pub fn finish(mut self) -> Said {
        if self.cut_off {
            return Said::CutOff;
        }
        if let Some(last) = self.pending.rest()
            && self.speak(&last) == Said::CutOff
        {
            return Said::CutOff;
        }

        match self.speaking.take() {
            Some(mut s) => {
                // Closing the input is what tells piper the answer is over. Without it the
                // stream waits forever for a sentence that is never coming.
                s.close_input();
                wait_out(&mut s, self.mic, &mut self.barge)
            }
            None => Said::Fully,
        }
    }

    /// Hands one sentence to the voice.
    ///
    /// The voice is opened once and kept open, so a sentence is a line written to something
    /// already running rather than a new pair of processes. Piper takes 0.29 seconds to load
    /// its model, measured, and paying that per sentence put an audible gap between every one
    /// of them.
    fn speak(&mut self, sentence: &str) -> Said {
        if !self.mouth.duplex {
            // No canceller, so the microphone must be off for the whole sentence and nothing
            // can interrupt. A stream that stays open would keep it deaf between sentences
            // too, which is worse, so this path still speaks one at a time.
            if let Err(e) = self.mic.deaf_while(|| self.mouth.voice.say(sentence)) {
                eprintln!("could not speak: {e}");
            }
            return Said::Fully;
        }

        // Opened on the first sentence, not before the answer, because a voice held open with
        // nothing to say holds the audio device against everything else on the machine.
        if self.speaking.is_none() {
            match self.mouth.voice.open() {
                Ok(v) => self.speaking = Some(v),
                Err(e) => {
                    eprintln!("could not open a voice: {e}");
                    return Said::Fully;
                }
            }
        }

        if let Some(voice) = self.speaking.as_mut()
            && let Err(e) = voice.add(sentence)
        {
            eprintln!("could not speak: {e}");
        }

        // Checked after handing it over rather than before, so the next sentence is already
        // queued and the words run together the way speech does.
        self.watch_for_interruption()
    }

    /// Watches for somebody talking over him, without waiting for the sentence to end.
    ///
    /// The old version waited out each sentence before queueing the next, which is what put a
    /// gap between them. Now the queue runs ahead and this only looks for a reason to stop.
    fn watch_for_interruption(&mut self) -> Said {
        if self.barge.saw(self.mic.recent(RECENT_SECS)) {
            if let Some(v) = self.speaking.as_mut() {
                v.stop();
            }
            self.cut_off = true;
            return Said::CutOff;
        }
        Said::Fully
    }
}
