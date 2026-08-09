//! Letting Carl write something down mid answer.
//!
//! Carl is supposed to be a helper that remembers, and until now his memory only ever got
//! written by hand or by a summary at the end of a spoken conversation. In practice the
//! directory stayed empty, so the one thing he was for did not happen.
//!
//! The obvious fix, asking Claude after every turn whether anything was worth keeping, costs
//! a second model call on every single message to answer "no" almost every time.
//!
//! So Carl writes the note himself, inside the answer he was already giving:
//!
//! ```text
//! Go for logistics science next.
//! [remember] JJ is playing Factorio, currently on red and green science.
//! ```
//!
//! The line is taken out before the answer is shown, spoken or recorded, and what is left
//! becomes a note. It costs nothing, it works on every surface at once, and Carl decides,
//! which is the only participant who knows whether something mattered.
//!
//! Pure text in, text and notes out. No files and no model.

/// What Carl writes at the start of a line to keep something.
pub const MARKER: &str = "[remember]";

/// Splits an answer into what to say and what to keep.
///
/// The marker only counts at the start of a line. Anywhere else it is Carl quoting the syntax
/// while explaining it, which he does, because he is told about it in his own brief.
pub fn split(answer: &str) -> (String, Vec<String>) {
    let mut said = Vec::new();
    let mut notes = Vec::new();

    for line in answer.lines() {
        match line.trim_start().strip_prefix(MARKER) {
            Some(note) => {
                let note = note.trim();
                // An empty marker keeps nothing. Writing a blank note would leave a file that
                // costs context on every future turn and says nothing.
                if !note.is_empty() {
                    notes.push(note.to_string());
                }
            }
            None => said.push(line),
        }
    }

    // Trailing blank lines are usually where the marker was, and an answer that ends in
    // whitespace reads as though it were cut off.
    let text = said.join("\n").trim_end().to_string();
    (text, notes)
}

/// Whether a line would be taken as a note, for the parts of speech that have to skip it
/// before the whole line has arrived.
pub fn is_note(line: &str) -> bool {
    line.trim_start().starts_with(MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_with_no_marker_is_untouched() {
        let (text, notes) = split("Go for logistics science next.");
        assert_eq!(text, "Go for logistics science next.");
        assert!(notes.is_empty());
    }

    /// The whole point. The note is kept and the person never sees the marker.
    #[test]
    fn a_note_is_taken_out_of_the_answer() {
        let (text, notes) = split(
            "Go for logistics science next.\n[remember] JJ is playing Factorio, on red and green.",
        );
        assert_eq!(text, "Go for logistics science next.");
        assert_eq!(notes, vec!["JJ is playing Factorio, on red and green."]);
    }

    #[test]
    fn several_notes_are_all_kept() {
        let (text, notes) = split("Sure.\n[remember] one\nmiddle\n[remember] two");
        assert_eq!(text, "Sure.\nmiddle");
        assert_eq!(notes, vec!["one", "two"]);
    }

    /// Carl is told about this syntax in his own brief, so he will sometimes explain it. A
    /// mention of the marker inside a sentence is him talking, not him writing a note.
    #[test]
    fn the_marker_only_counts_at_the_start_of_a_line() {
        let (text, notes) =
            split("You can write [remember] at the start of a line to keep something.");
        assert!(notes.is_empty(), "{notes:?}");
        assert!(text.contains("[remember]"));
    }

    /// A blank note would leave a file that costs context on every future turn and says
    /// nothing at all.
    #[test]
    fn an_empty_note_keeps_nothing() {
        let (text, notes) = split("Fine.\n[remember]\n[remember]   ");
        assert_eq!(text, "Fine.");
        assert!(notes.is_empty());
    }

    /// An answer that is only a note still has to leave something sayable behind, or Carl
    /// goes silent having apparently answered.
    #[test]
    fn an_answer_that_is_only_a_note_comes_back_empty_rather_than_odd() {
        let (text, notes) = split("[remember] JJ prefers short answers.");
        assert_eq!(text, "");
        assert_eq!(notes, vec!["JJ prefers short answers."]);
    }

    #[test]
    fn indentation_does_not_hide_the_marker() {
        let (_, notes) = split("ok\n   [remember] indented");
        assert_eq!(notes, vec!["indented"]);
    }

    /// An answer ending in whitespace where the marker was reads as though it were cut off.
    #[test]
    fn the_answer_does_not_end_in_the_gap_the_note_left() {
        let (text, _) = split("All done.\n\n[remember] something\n");
        assert_eq!(text, "All done.");
    }

    #[test]
    fn a_line_can_be_recognised_before_it_is_split() {
        assert!(is_note("[remember] x"));
        assert!(is_note("  [remember] x"));
        assert!(!is_note("say [remember] x"));
        assert!(!is_note("ordinary line"));
    }
}
