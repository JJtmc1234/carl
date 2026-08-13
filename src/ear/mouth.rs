//! Speaking, and stopping when you speak over him.
//!
//! Carl's voice is reachable only through this type, and this type demands the microphone as
//! an argument. It does not use it to hear anything. It uses it to know when to stop, and in
//! the fallback case to switch itself off. A spoken line that forgot either would sound like
//! Carl talking to himself, and it would fail silently, so the wrong version is made
//! impossible to write rather than left to memory.

use std::time::Duration;

use carl::Voice;
use carl::audio::Mic;
use carl::speech::Speaking;

use crate::Result;

/// How often to check whether you have started talking. Short enough that Carl stops within
/// a syllable or two of being cut off.
pub(super) const CHECK: Duration = Duration::from_millis(100);

/// How much of the recent past counts as "right now" when watching for an interruption.
pub(super) const RECENT_SECS: f32 = 0.3;

/// How many checks in a row must be loud before Carl stops.
///
/// One is far too eager. A door, a cough or a chair leg all clear the threshold for a single
/// tick, and being cut off mid answer by a chair is worse than not being interruptible.
const SUSTAINED: u32 = 3;

/// How much louder than the room an interruption has to be.
///
/// Above the hush threshold rather than equal to it. Hush decides when you have finished
/// speaking, which should be sensitive. This decides whether to abandon an answer, which
/// should not be.
const OVER_ROOM: f32 = 2.5;

/// Decides when talking over Carl is deliberate rather than a noise.
///
/// Its own type so the rule can be tested without a microphone, a speaker or a room. Every
/// threshold in Carl that lived only inside a loop has needed changing once someone actually
/// used it, and the ones that were pure functions were the cheap ones to change.
pub(super) struct Barge {
    over: f32,
    ticks: u32,
}

impl Barge {
    pub(super) fn new(hush: f32) -> Self {
        Self {
            over: hush * OVER_ROOM,
            ticks: 0,
        }
    }

    /// Feeds in one check. True once it has been loud long enough to mean it.
    pub(super) fn saw(&mut self, level: f32) -> bool {
        if level > self.over {
            self.ticks += 1;
        } else {
            self.ticks = 0;
        }
        self.ticks >= SUSTAINED
    }
}

pub struct Mouth {
    pub(super) voice: Voice,
    /// Whether Carl can hear while talking. Only true with the echo canceller running, since
    /// otherwise everything the microphone hears during a reply is Carl himself.
    pub(super) duplex: bool,
}

/// What ended a spoken line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Said {
    /// He finished the sentence.
    Fully,
    /// You started talking, so he stopped.
    CutOff,
}

impl Mouth {
    pub fn voice(&self) -> &Voice {
        &self.voice
    }

    pub fn new(voice: Voice, duplex: bool) -> Self {
        Self { voice, duplex }
    }

    /// Says something out loud. `hush` is the level this room counts as quiet.
    pub fn say(&self, mic: &Mic, text: &str, hush: f32) -> Result<Said> {
        if !self.duplex {
            // No canceller, so listening while speaking only ever hears Carl. Half duplex is
            // the honest fallback: he finishes, and nothing said meanwhile is heard.
            mic.deaf_while(|| self.voice.say(text))?;
            return Ok(Said::Fully);
        }

        let Some(mut speaking) = self.voice.start(text)? else {
            return Ok(Said::Fully);
        };

        let mut barge = Barge::new(hush);
        Ok(wait_out(&mut speaking, mic, &mut barge))
    }
}

/// Waits for a sentence to finish playing, stopping early if you talk over it.
///
/// Deliberately does not forget the microphone window afterwards. What you said to interrupt
/// him is the start of your next sentence, and throwing it away would make you repeat the
/// first word of every interruption.
pub(super) fn wait_out(speaking: &mut Speaking, mic: &Mic, barge: &mut Barge) -> Said {
    loop {
        if speaking.done() {
            return Said::Fully;
        }
        std::thread::sleep(CHECK);

        if barge.saw(mic.recent(RECENT_SECS)) {
            speaking.stop();
            return Said::CutOff;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUSH: f32 = 0.04;
    const SPEECH: f32 = 0.30;
    const ROOM: f32 = 0.03;

    /// A door, a cough or a chair leg all clear the threshold for one check. Being cut off
    /// mid answer by a chair is worse than not being interruptible at all.
    #[test]
    fn one_loud_moment_is_not_an_interruption() {
        let mut b = Barge::new(HUSH);
        assert!(!b.saw(SPEECH));
    }

    #[test]
    fn talking_over_him_for_long_enough_is() {
        let mut b = Barge::new(HUSH);
        let mut cut = false;
        for _ in 0..SUSTAINED {
            cut = b.saw(SPEECH);
        }
        assert!(cut, "sustained speech should stop him");
    }

    /// Two spikes with a quiet moment between them are two noises, not a sentence.
    #[test]
    fn a_gap_starts_the_count_again() {
        let mut b = Barge::new(HUSH);
        for _ in 0..SUSTAINED - 1 {
            assert!(!b.saw(SPEECH));
        }
        assert!(!b.saw(ROOM), "quiet must reset");
        assert!(!b.saw(SPEECH), "and the count starts from one again");
    }

    /// The interruption threshold sits above the one that decides you stopped talking. If
    /// they were equal, the level that ends your own sentence would also abandon his answer,
    /// and Carl would cut himself off on the tail of the question you just asked.
    #[test]
    fn merely_not_being_silent_is_not_enough_to_interrupt() {
        let mut b = Barge::new(HUSH);
        let just_over_hush = HUSH * 1.2;
        for _ in 0..SUSTAINED * 2 {
            assert!(
                !b.saw(just_over_hush),
                "this level ends a sentence, it must not abandon an answer"
            );
        }
    }

    /// A loud room raises the bar with it, rather than making Carl impossible to finish.
    #[test]
    fn the_bar_moves_with_the_room() {
        let quiet = Barge::new(0.01);
        let loud = Barge::new(0.20);
        assert!(loud.over > quiet.over);
    }
}
