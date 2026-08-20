//! What of a streamed answer can safely be put on a terminal yet.
//!
//! A terminal is the one surface that cannot take anything back. Slack rewrites the whole
//! message through `api.update`, and the panel redraws, so both can print something and change
//! their mind. Bytes written to a tty are gone.
//!
//! That matters because [`remember::split`](crate::remember) removes memory lines from the
//! answer, so the visible text can get *shorter* as more arrives. The streaming loops tracked
//! how much had been shown as a byte offset into the visible text and never corrected it when
//! that happened, so after one note line every later slice cut into the middle of the answer.
//! A partial marker was left on screen, the words after it were never printed, and what came
//! after that was mangled. The recorded answer was right the whole time, so nothing noticed.
//!
//! One place rather than two. The same loop was written out in `repl.rs` and in `main.rs`, and
//! the same bug was in both. See bug 18.

use crate::remember::{self, FORGET, MARKER, SEEN};

/// Whether a line that has not ended yet could still turn out to be a memory line.
///
/// This is the only text that can ever be retracted, so it is the only text worth holding
/// back. A marker spans several tokens, so a stream boundary lands inside one often, and
/// `split` cannot judge a line until it ends.
///
/// Deliberately narrow. Holding back every unfinished line would mean a paragraph appears all
/// at once when its newline arrives, which is most of what streaming is for. Ordinary prose
/// does not begin with a bracket, so in practice this holds back a few characters and only
/// when there is genuine doubt.
fn may_yet_be_a_note(line: &str) -> bool {
    let t = line.trim_start();
    [MARKER, FORGET, SEEN]
        .iter()
        .any(|m| m.starts_with(t) || t.starts_with(m))
}

/// How much of an answer has been put on the screen.
///
/// Counted in whole lines rather than in bytes. A byte offset into a string that can shrink is
/// what caused bug 18, and clamping it would stop the panic without stopping the mangling,
/// because a clamped offset still points at the wrong place.
#[derive(Debug, Default)]
pub struct Shown {
    /// Complete lines already written, including their newline.
    lines: usize,
    /// Bytes of the current unfinished line already written, reset when it completes.
    partial: usize,
}

impl Shown {
    /// The next text to print, given everything received so far.
    ///
    /// A trailing line that has not ended is printed unless it might still be a note.
    pub fn next(&mut self, whole: &str) -> String {
        self.take(whole, false)
    }

    /// Everything left, for when the stream has ended and no more newlines are coming.
    ///
    /// Held back text has to go somewhere. A last line that turned out not to be a note would
    /// otherwise be dropped from the screen while sitting in the recorded answer, which is the
    /// same class of mistake in the other direction.
    pub fn rest(&mut self, whole: &str) -> String {
        self.take(whole, true)
    }

    fn take(&mut self, whole: &str, ended: bool) -> String {
        let visible = remember::split(whole).text;
        let mut out = String::new();

        // `split_inclusive` keeps the newline on the line it belongs to, so a line that has
        // arrived complete is exactly one that ends in one.
        for line in visible.split_inclusive('\n').skip(self.lines) {
            // `get` rather than a slice. `split` trims the end of the whole answer, so the
            // last line can in principle come back shorter than what was already shown, and a
            // terminal printing nothing is better than a panic in the middle of an answer.
            let fresh = line.get(self.partial..).unwrap_or("");
            if line.ends_with('\n') {
                out.push_str(fresh);
                self.lines += 1;
                self.partial = 0;
            } else if ended || !may_yet_be_a_note(line) {
                // Not counted as a line. It has no newline, so the next call sees the same
                // line again and longer, and prints only what was added to it.
                out.push_str(fresh);
                self.partial = line.len();
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a terminal ends up holding, given the answer arriving in these pieces.
    fn on_screen(chunks: &[&str]) -> String {
        let mut whole = String::new();
        let mut shown = Shown::default();
        let mut screen = String::new();
        for c in chunks {
            whole.push_str(c);
            screen.push_str(&shown.next(&whole));
        }
        screen.push_str(&shown.rest(&whole));
        screen
    }

    /// What the answer actually says, which the terminal has to match.
    fn recorded(chunks: &[&str]) -> String {
        remember::split(&chunks.concat()).text
    }

    /// The first reported case. A note in the middle, with a delta boundary inside the marker.
    ///
    /// Against the old code the terminal held "Answer.\n[remember." while the recorded answer
    /// said "Answer.\nMore text.", so the words between were never printed at all.
    #[test]
    fn a_note_in_the_middle_does_not_mangle_what_follows() {
        let chunks = [
            "Answer.\n",
            "[",
            "remember",
            "] JJ likes short answers",
            "\n",
            "More text.",
        ];
        assert_eq!(on_screen(&chunks), recorded(&chunks));
        assert_eq!(on_screen(&chunks), "Answer.\nMore text.");
    }

    /// The second reported case, where the boundary falls inside the word rather than the
    /// bracket. Against the old code the terminal held "Answer.\n[rememst of the answer."
    #[test]
    fn a_marker_split_across_deltas_never_reaches_the_screen() {
        let chunks = [
            "Answer.\n[remem",
            "ber] a fact\n",
            "The rest of the answer.",
        ];
        assert_eq!(on_screen(&chunks), recorded(&chunks));
        assert_eq!(on_screen(&chunks), "Answer.\nThe rest of the answer.");
    }

    /// Every marker retracts, not only the one that was reported.
    #[test]
    fn forget_and_seen_lines_are_held_back_too() {
        for marker in [MARKER, FORGET, SEEN] {
            let line = format!("{marker} something\n");
            let chunks = ["Before.\n", &line[..3], &line[3..], "After."];
            assert_eq!(on_screen(&chunks), recorded(&chunks), "{marker}");
            assert_eq!(on_screen(&chunks), "Before.\nAfter.", "{marker}");
        }
    }

    /// Ordinary prose must still arrive as it is written, or this stops being streaming.
    ///
    /// The narrow rule earns its keep here. Holding back every unfinished line would make a
    /// paragraph appear all at once when its newline arrives, and a paragraph is the normal
    /// shape of an answer.
    #[test]
    fn ordinary_text_is_not_held_back_waiting_for_a_newline() {
        let mut whole = String::new();
        let mut shown = Shown::default();
        let mut screen = String::new();

        for chunk in ["Go for ", "logistics ", "science"] {
            whole.push_str(chunk);
            screen.push_str(&shown.next(&whole));
            // Everything the answer says so far is already on screen, mid line and with no
            // newline in sight. `split` trims the end of the answer, so a trailing space
            // arrives with the next chunk rather than with its own, which is what this
            // compares against rather than against the raw chunks.
            assert_eq!(
                screen,
                remember::split(&whole).text,
                "held back after {chunk:?}"
            );
        }
    }

    /// A last line that turns out not to be a note still has to reach the screen.
    ///
    /// The same mistake in the other direction. It would sit in the recorded answer and never
    /// be printed, which is what the old code did to the text after a note.
    #[test]
    fn a_held_back_last_line_is_flushed_when_the_stream_ends() {
        // Never completed, so nothing can ever decide it is not a note except the end.
        let chunks = ["Answer.\n", "[rem"];
        assert_eq!(on_screen(&chunks), "Answer.\n[rem");
        assert_eq!(on_screen(&chunks), recorded(&chunks));
    }

    #[test]
    fn only_a_real_marker_prefix_is_doubtful() {
        for held in ["[", "[rem", "[remember]", " [forget] a", "[se"] {
            assert!(may_yet_be_a_note(held), "{held:?}");
        }
        for shown in [
            "[x",
            "Answer.",
            "[[",
            "remember]",
            "a [remember] inside a sentence",
        ] {
            assert!(!may_yet_be_a_note(shown), "{shown:?}");
        }
    }
}
