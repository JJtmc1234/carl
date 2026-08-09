//! Showing an answer being written, in a place that cannot stream.
//!
//! Claude takes five to twenty five seconds. The voice does not suffer for that, because a
//! voice arriving late is obviously still on its way. Slack shows nothing at all until the
//! whole answer lands, so a question sits there looking ignored and then is suddenly
//! answered, and in between the only honest reading is that Carl is broken.
//!
//! Slack has no way to stream into a message. What it has is `chat.update`, so the message is
//! posted immediately and rewritten as the words arrive.
//!
//! The whole problem is how often to rewrite. Slack rate limits edits to roughly one a second
//! per channel, and going over does not fail politely: it returns `ratelimited` and the
//! answer stops updating, which is worse than never having streamed at all. So the pacing is
//! its own type, with its own tests, and it is deliberately conservative.

use std::time::{Duration, Instant};

/// How long to wait between rewrites.
///
/// Slack allows about one a second. This sits above that rather than at it, because the
/// limit is per channel and Carl may be answering in two threads at once, and because the
/// cost of being too slow is a slightly jerkier answer while the cost of being too fast is
/// the answer freezing.
const GAP: Duration = Duration::from_millis(1_500);

/// How much new text is worth a rewrite on its own.
///
/// Without this, a slow answer rewrites the message every 1.5 seconds to add three words,
/// which flickers for no benefit.
const WORTH_IT: usize = 40;

/// Decides when a message should be rewritten.
pub struct Pace {
    last: Instant,
    shown: usize,
}

impl Pace {
    /// Starts having just posted `shown` characters.
    pub fn started_with(shown: usize) -> Self {
        Self {
            last: Instant::now(),
            shown,
        }
    }

    /// Whether it is worth rewriting now, given how much text there is.
    ///
    /// Takes the length rather than the text, because the only question is whether enough has
    /// changed, and passing the whole answer in would invite comparing it.
    pub fn should_update(&mut self, len: usize) -> bool {
        let grown = len.saturating_sub(self.shown);
        if grown == 0 {
            return false;
        }
        if grown < WORTH_IT && self.last.elapsed() < GAP * 2 {
            return false;
        }
        if self.last.elapsed() < GAP {
            return false;
        }
        self.last = Instant::now();
        self.shown = len;
        true
    }
}

/// What to show before there are any words.
///
/// Says what he is doing rather than showing a spinner. "Looking at your screen" and
/// "thinking" are different waits and it is worth knowing which one you are in.
pub fn placeholder(looking: bool) -> &'static str {
    if looking {
        "_looking at your screen..._"
    } else {
        "_thinking..._"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_new_is_never_worth_a_rewrite() {
        let mut p = Pace::started_with(100);
        assert!(!p.should_update(100));
        assert!(!p.should_update(50), "shrinking is not growing either");
    }

    /// Going over Slack's limit does not fail politely. It returns ratelimited and the answer
    /// stops updating, which is worse than never having streamed.
    #[test]
    fn two_rewrites_cannot_happen_back_to_back() {
        let mut p = Pace::started_with(0);
        assert!(!p.should_update(1_000), "too soon after starting");
    }

    /// A slow answer must not rewrite the whole message to add three words.
    #[test]
    fn a_trickle_of_new_text_waits_longer_than_a_flood() {
        let mut p = Pace::started_with(0);
        p.last = Instant::now() - GAP - Duration::from_millis(100);

        assert!(
            !p.should_update(WORTH_IT - 1),
            "a few words just after the gap should wait"
        );

        let mut q = Pace::started_with(0);
        q.last = Instant::now() - GAP - Duration::from_millis(100);
        assert!(
            q.should_update(WORTH_IT + 1),
            "a real chunk of text should go out"
        );
    }

    /// A trickle still has to appear eventually, or a slow answer never updates at all.
    #[test]
    fn a_trickle_goes_out_once_it_has_waited_long_enough() {
        let mut p = Pace::started_with(0);
        p.last = Instant::now() - GAP * 2 - Duration::from_millis(100);
        assert!(
            p.should_update(5),
            "even five characters, after long enough"
        );
    }

    /// After a rewrite the clock and the mark both move, or the next call fires immediately.
    #[test]
    fn a_rewrite_resets_both_the_clock_and_the_mark() {
        let mut p = Pace::started_with(0);
        p.last = Instant::now() - GAP * 3;
        assert!(p.should_update(500));
        assert!(!p.should_update(500), "same length, nothing new");
        assert!(!p.should_update(600), "and too soon regardless");
    }

    /// The two waits are different and worth telling apart. A screenshot takes a moment and a
    /// flash, and knowing that is happening is better than a spinner.
    #[test]
    fn the_placeholder_says_which_wait_this_is() {
        assert!(placeholder(true).contains("screen"));
        assert!(placeholder(false).contains("thinking"));
        assert_ne!(placeholder(true), placeholder(false));
    }
}
