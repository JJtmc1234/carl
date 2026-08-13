//! Noticing that an answer did not land.
//!
//! Carl has no idea when he gets something wrong. He answers, the answer is useless, and the
//! next question arrives as though the last exchange had gone fine. The person who knows it
//! went wrong is the one who has to say so, and they usually already did, immediately, in the
//! next thing they said.
//!
//! Two signals, and both are ordinary conversation rather than anything anybody has to learn.
//!
//! Saying no. "No, I meant the other one." A person only says that when the answer missed.
//!
//! Saying it again. Asking the same thing twice in a row means the first answer did not
//! count, whatever the words were.
//!
//! The point is not to score Carl. It is that the next turn can be told the last one missed,
//! which costs nothing and is the difference between a second wrong answer and a different
//! one.

/// Why an answer looks like it missed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missed {
    /// They said no, or corrected him outright.
    Corrected,
    /// They asked the same thing again.
    Repeated,
}

impl Missed {
    /// What to tell Carl about it, in the next turn.
    pub fn note(self) -> &'static str {
        match self {
            Self::Corrected => {
                "Your last answer was wrong or missed the point, because they are correcting \
                 you. Do not repeat it or defend it. Work out what they actually meant, say \
                 the new answer, and keep it short."
            }
            Self::Repeated => {
                "They just asked the same thing again, which means your last answer did not \
                 land. Do not say it again in different words. Either answer differently or \
                 ask exactly what is unclear, in one sentence."
            }
        }
    }
}

/// Phrases that only get said when an answer missed.
///
/// Whole phrases rather than single words, because the single words are ambiguous and the
/// phrases are not. "No" on its own is usually an answer to a question.
const CORRECTIONS: &[&str] = &[
    "no i meant",
    "i meant",
    "that is not what i",
    "thats not what i",
    "not what i meant",
    "that is wrong",
    "thats wrong",
    "you are wrong",
    "youre wrong",
    "that is not right",
    "thats not right",
    "no that is",
    "no thats",
    "not that",
    "you are not listening",
    "youre not listening",
    "i did not ask",
    "i didnt ask",
    "wrong game",
    "no i asked",
];

/// Whether the last thing Carl said was a question.
///
/// Matters because "no" answers a question and rejects a statement, and the two are opposite.
/// Carl asks plenty of questions, so without this every "no" would read as being told off.
pub fn is_a_question(said: &str) -> bool {
    let t = said.trim_end();
    t.ends_with('?')
}

/// Whether this reads as being told the last answer missed.
///
/// `carl_asked` is what Carl said last, so a bare "no" can be told apart from a correction.
pub fn missed(said: &str, carl_asked: &str) -> Option<Missed> {
    let now = normalise(said);
    if now.is_empty() {
        return None;
    }

    // An explicit correction counts whatever came before it.
    if CORRECTIONS.iter().any(|c| now.contains(c)) {
        return Some(Missed::Corrected);
    }

    // A bare no, only when Carl was not asking anything. Answering "no" to "do you have coal"
    // is a conversation, not a complaint.
    if !is_a_question(carl_asked) && matches!(now.as_str(), "no" | "nope" | "no no" | "wrong") {
        return Some(Missed::Corrected);
    }

    None
}

/// Whether this is the same question again.
///
/// Compared as sets of words rather than as strings, because nobody repeats themselves
/// exactly. "What should I research next" and "so what should I research" are the same
/// question asked twice.
pub fn repeated(said: &str, said_before: &str) -> Option<Missed> {
    let now: Vec<String> = normalise(said)
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let before: Vec<String> = normalise(said_before)
        .split_whitespace()
        .map(str::to_owned)
        .collect();

    // Too short to judge. "Yes" twice is not a repeated question.
    if now.len() < 3 || before.len() < 3 {
        return None;
    }

    let shared = now.iter().filter(|w| before.contains(w)).count();
    let overlap = shared as f32 / now.len().max(before.len()) as f32;

    (overlap >= 0.7).then_some(Missed::Repeated)
}

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_outright_correction_is_noticed() {
        for said in [
            "no I meant the other furnace",
            "that's not what I asked",
            "you're wrong, it is copper",
            "no that's the wrong game",
        ] {
            assert_eq!(
                missed(said, "Go for steel."),
                Some(Missed::Corrected),
                "should have noticed: {said}"
            );
        }
    }

    /// The one that would go wrong constantly. Carl asks plenty of questions, and "no" is the
    /// ordinary answer to one, not a complaint about the last one.
    #[test]
    fn no_to_a_question_is_an_answer_and_not_a_telling_off() {
        assert_eq!(missed("no", "Do you have coal yet?"), None);
        assert_eq!(missed("nope", "Have you unlocked steel?"), None);

        // The same word, after a statement, is a complaint.
        assert_eq!(missed("no", "Go for steel next."), Some(Missed::Corrected));
    }

    #[test]
    fn ordinary_conversation_is_left_alone() {
        for said in [
            "what should I research next",
            "there is no coal on this patch",
            "I know, I already did that",
            "thanks that worked",
        ] {
            assert_eq!(
                missed(said, "Go for steel."),
                None,
                "false alarm on: {said}"
            );
        }
    }

    /// Nobody repeats themselves word for word, so this compares what was said rather than
    /// how it was said.
    #[test]
    fn asking_the_same_thing_again_is_noticed() {
        assert_eq!(
            repeated(
                "so what should I research next",
                "what should I research next"
            ),
            Some(Missed::Repeated)
        );
    }

    #[test]
    fn a_different_question_is_not_a_repeat() {
        assert_eq!(
            repeated("how much coal do I need", "what should I research next"),
            None
        );
    }

    /// Short answers repeat constantly in ordinary talk and mean nothing.
    #[test]
    fn short_things_are_not_repeats() {
        assert_eq!(repeated("yes", "yes"), None);
        assert_eq!(repeated("ok", "ok"), None);
    }

    /// The note has to tell Carl what to do differently, not just that he failed. Being told
    /// you were wrong and nothing else invites saying the same thing more firmly.
    #[test]
    fn the_note_says_what_to_do_instead() {
        for m in [Missed::Corrected, Missed::Repeated] {
            let n = m.note().to_lowercase();
            assert!(n.contains("do not"), "{n}");
            assert!(
                n.contains("short") || n.contains("one sentence") || n.contains("differently"),
                "{n}"
            );
        }
    }

    #[test]
    fn a_question_is_recognised_by_its_mark() {
        assert!(is_a_question("Do you have coal?"));
        assert!(is_a_question("Do you have coal? "));
        assert!(!is_a_question("Go for steel."));
    }
}
