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

/// And to drop something he kept before.
///
/// Needed because a wrong note is worse than a missing one. It comes back on every single
/// turn, forever, and gets stated with confidence. Replacing is not enough on its own: a
/// corrected fact is usually worded differently, so it lands under a different name and the
/// old one sits there contradicting it.
pub const FORGET: &str = "[forget]";

/// What Carl decided to do with his memory this turn.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// The answer with the memory lines taken out.
    pub text: String,
    pub keep: Vec<String>,
    /// Notes to drop, named either by their filename or by what they said.
    pub drop: Vec<String>,
}

/// A short, stable filename for a note.
///
/// Named from the note itself rather than from the clock, so writing the same fact twice
/// replaces it instead of accumulating near duplicates that each cost context on every future
/// turn forever.
///
/// Shared with forgetting on purpose. If naming and forgetting disagreed by one character,
/// `[forget]` would silently do nothing and the wrong note would stay.
pub fn note_name(note: &str) -> String {
    let slug = note
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .to_lowercase()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() { String::new() } else { slug }
}

/// Splits an answer into what to say and what to keep.
///
/// The marker only counts at the start of a line. Anywhere else it is Carl quoting the syntax
/// while explaining it, which he does, because he is told about it in his own brief.
pub fn split(answer: &str) -> Kept {
    let mut said = Vec::new();
    let mut kept = Kept::default();

    for line in answer.lines() {
        let t = line.trim_start();

        if let Some(note) = t.strip_prefix(MARKER) {
            // An empty marker keeps nothing. A blank note would be a file that costs context
            // on every future turn and says nothing at all.
            let note = note.trim();
            if !note.is_empty() {
                kept.keep.push(note.to_string());
            }
        } else if let Some(note) = t.strip_prefix(FORGET) {
            let note = note.trim();
            if !note.is_empty() {
                kept.drop.push(note.to_string());
            }
        } else {
            said.push(line);
        }
    }

    // Trailing blank lines are usually where a marker was, and an answer ending in whitespace
    // reads as though it were cut off.
    kept.text = said.join("\n").trim_end().to_string();
    kept
}

/// Whether a line would be taken as a note, for the parts of speech that have to skip it
/// before the whole line has arrived.
pub fn is_note(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with(MARKER) || t.starts_with(FORGET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_with_no_marker_is_untouched() {
        let Kept {
            text, keep: notes, ..
        } = split("Go for logistics science next.");
        assert_eq!(text, "Go for logistics science next.");
        assert!(notes.is_empty());
    }

    /// The whole point. The note is kept and the person never sees the marker.
    #[test]
    fn a_note_is_taken_out_of_the_answer() {
        let Kept {
            text, keep: notes, ..
        } = split(
            "Go for logistics science next.\n[remember] JJ is playing Factorio, on red and green.",
        );
        assert_eq!(text, "Go for logistics science next.");
        assert_eq!(notes, vec!["JJ is playing Factorio, on red and green."]);
    }

    #[test]
    fn several_notes_are_all_kept() {
        let Kept {
            text, keep: notes, ..
        } = split("Sure.\n[remember] one\nmiddle\n[remember] two");
        assert_eq!(text, "Sure.\nmiddle");
        assert_eq!(notes, vec!["one", "two"]);
    }

    /// Carl is told about this syntax in his own brief, so he will sometimes explain it. A
    /// mention of the marker inside a sentence is him talking, not him writing a note.
    #[test]
    fn the_marker_only_counts_at_the_start_of_a_line() {
        let Kept {
            text, keep: notes, ..
        } = split("You can write [remember] at the start of a line to keep something.");
        assert!(notes.is_empty(), "{notes:?}");
        assert!(text.contains("[remember]"));
    }

    /// A blank note would leave a file that costs context on every future turn and says
    /// nothing at all.
    #[test]
    fn an_empty_note_keeps_nothing() {
        let Kept {
            text, keep: notes, ..
        } = split("Fine.\n[remember]\n[remember]   ");
        assert_eq!(text, "Fine.");
        assert!(notes.is_empty());
    }

    /// An answer that is only a note still has to leave something sayable behind, or Carl
    /// goes silent having apparently answered.
    #[test]
    fn an_answer_that_is_only_a_note_comes_back_empty_rather_than_odd() {
        let Kept {
            text, keep: notes, ..
        } = split("[remember] JJ prefers short answers.");
        assert_eq!(text, "");
        assert_eq!(notes, vec!["JJ prefers short answers."]);
    }

    #[test]
    fn indentation_does_not_hide_the_marker() {
        let Kept { keep: notes, .. } = split("ok\n   [remember] indented");
        assert_eq!(notes, vec!["indented"]);
    }

    /// An answer ending in whitespace where the marker was reads as though it were cut off.
    #[test]
    fn the_answer_does_not_end_in_the_gap_the_note_left() {
        let Kept { text, .. } = split("All done.\n\n[remember] something\n");
        assert_eq!(text, "All done.");
    }

    /// A wrong note is worse than a missing one, because it comes back on every turn and is
    /// stated with confidence.
    #[test]
    fn a_note_can_be_dropped() {
        let k = split("You are right, sorry.\n[forget] JJ prefers two sentence answers");
        assert_eq!(k.text, "You are right, sorry.");
        assert_eq!(k.drop, vec!["JJ prefers two sentence answers"]);
        assert!(k.keep.is_empty());
    }

    /// Correcting a fact is a drop and a keep in one breath, and both have to survive.
    #[test]
    fn a_correction_is_both_at_once() {
        let k = split(
            "Noted.\n[forget] JJ is on red and green science\n[remember] JJ is on blue science",
        );
        assert_eq!(k.text, "Noted.");
        assert_eq!(k.drop, vec!["JJ is on red and green science"]);
        assert_eq!(k.keep, vec!["JJ is on blue science"]);
    }

    /// The naming rule is shared with forgetting on purpose. If the two disagreed by one
    /// character, [forget] would quietly do nothing and the wrong note would stay.
    #[test]
    fn a_note_and_a_forget_of_the_same_words_agree_on_the_name() {
        let fact = "JJ prefers two sentence answers";
        assert_eq!(note_name(fact), note_name(&format!("  {fact}!  ")));
        assert_eq!(note_name(fact), "jj-prefers-two-sentence-answers");
    }

    /// A filename becomes a path, so nothing path shaped may survive naming.
    #[test]
    fn a_note_cannot_smuggle_a_path_into_a_filename() {
        let n = note_name("../../etc/passwd is interesting");
        assert!(!n.contains('/'), "{n}");
        assert!(!n.contains(".."), "{n}");
    }

    #[test]
    fn a_note_with_no_letters_at_all_has_no_name() {
        assert_eq!(note_name("!!! ???"), "");
    }

    #[test]
    fn a_line_can_be_recognised_before_it_is_split() {
        assert!(is_note("[remember] x"));
        assert!(is_note("  [remember] x"));
        assert!(is_note("[forget] x"), "a forget must not be spoken either");
        assert!(!is_note("say [remember] x"));
        assert!(!is_note("ordinary line"));
    }
}
