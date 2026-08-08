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

use super::mouth::{Barge, Mouth, Said, wait_out};

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
            Some(mut s) => wait_out(&mut s, self.mic, &mut self.barge),
            None => Said::Fully,
        }
    }

    /// Queues one sentence, waiting out whatever is already playing.
    fn speak(&mut self, sentence: &str) -> Said {
        if let Some(mut current) = self.speaking.take()
            && wait_out(&mut current, self.mic, &mut self.barge) == Said::CutOff
        {
            self.cut_off = true;
            return Said::CutOff;
        }

        if self.mouth.duplex {
            match self.mouth.voice.start(sentence) {
                Ok(started) => self.speaking = started,
                // A player that will not start is not worth abandoning the answer over. The
                // text is still printed and still recorded.
                Err(e) => eprintln!("could not speak: {e}"),
            }
        } else if let Err(e) = self.mic.deaf_while(|| self.mouth.voice.say(sentence)) {
            // No canceller, so the microphone has to be off for the whole sentence. Nothing
            // can interrupt, and the next sentence simply waits.
            eprintln!("could not speak: {e}");
        }
        Said::Fully
    }
}
