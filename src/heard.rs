//! Deciding what a transcript means.
//!
//! Pure text in, decision out. No audio and no model, so every rule here is testable without
//! a microphone, which matters because these rules are the ones that decide whether a
//! recording is kept or destroyed.
//!
//! Whisper mishears "Carl" constantly. It writes Karl, Carol, call, and worse. Matching only
//! the exact spelling means the wake word fails often enough to be useless, so the variants
//! below are deliberate rather than sloppy.

/// What the listener should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Not for Carl. The recording is destroyed and nothing is kept.
    Nothing,
    /// The wake word, with whatever was said after it.
    ///
    /// People rarely stop at "Hey Carl". They say "Hey Carl, what do I do now", and treating
    /// that as a bare wake word would throw the question away and make them repeat it.
    Wake { question: Option<String> },
    /// Something said while Carl is already listening.
    Say(String),
    /// The end of the conversation.
    End,
}

/// Spellings whisper produces for "Carl". Ordered longest first so "hey carl" is preferred
/// over a shorter accidental match inside it.
const NAMES: &[&str] = &[
    "carl", "karl", "carle", "carol", "kall", "call", "cal", "kar",
];

const END_PHRASES: &[&str] = &[
    "end conversation",
    "end the conversation",
    "and conversation",
    "end conversion",
    "end convo",
    "goodbye carl",
    "bye carl",
];

/// Lowercase, strip punctuation, squash runs of spaces.
///
/// Whisper adds commas and full stops that would otherwise break a plain substring match, so
/// "Hey, Carl." and "hey carl" have to normalise to the same thing.
pub fn normalise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_space = true;
    for c in raw.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

/// Where the wake phrase sits in the text, if it is there at all.
fn find_wake(text: &str) -> Option<(usize, usize)> {
    for name in NAMES {
        for lead in ["hey ", "hi ", "ok ", "okay ", "hey there "] {
            let phrase = format!("{lead}{name}");
            if let Some(at) = text.find(&phrase) {
                return Some((at, at + phrase.len()));
            }
        }
    }
    None
}

/// Reads a transcript in light of whether Carl is already listening.
pub fn interpret(transcript: &str, listening: bool) -> Heard {
    let text = normalise(transcript);
    if text.is_empty() {
        return Heard::Nothing;
    }

    if listening {
        // Checked before anything else, so "end conversation" always lands even when it is
        // buried in a longer sentence.
        if END_PHRASES.iter().any(|p| text.contains(p)) {
            return Heard::End;
        }
        return Heard::Say(text);
    }

    match find_wake(&text) {
        Some((_, after)) => {
            let rest = text[after..].trim();
            Heard::Wake {
                question: if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                },
            }
        }
        // Not addressed to Carl. This is the branch that destroys the audio.
        None => Heard::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_and_case_do_not_matter() {
        assert_eq!(normalise("Hey, Carl!"), "hey carl");
        assert_eq!(normalise("  END   Conversation.  "), "end conversation");
    }

    #[test]
    fn the_plain_wake_word_wakes_him() {
        assert_eq!(interpret("Hey Carl", false), Heard::Wake { question: None });
    }

    /// People do not pause after the wake word. Dropping the question would make them repeat
    /// themselves every single time.
    #[test]
    fn a_question_on_the_same_breath_is_kept() {
        assert_eq!(
            interpret("Hey Carl, what do I do now?", false),
            Heard::Wake {
                question: Some("what do i do now".into())
            }
        );
    }

    /// Whisper mishears the name constantly, and a wake word that only works sometimes is
    /// worse than none.
    #[test]
    fn common_mishearings_still_wake_him() {
        for said in [
            "Hey Karl",
            "hey carol, show me your techs",
            "Okay Carl",
            "hi Carl",
            "Hey, call. What now?",
        ] {
            assert!(
                matches!(interpret(said, false), Heard::Wake { .. }),
                "{said:?} should have woken him"
            );
        }
    }

    /// The whole privacy promise. Anything not addressed to Carl is nothing, and the caller
    /// destroys the recording on this answer.
    #[test]
    fn ordinary_talk_is_nothing_at_all() {
        for said in [
            "so then I told him it was fine",
            "can you pass the salt",
            "I need more iron plates",
            "",
            "   ...   ",
            "Carl",
        ] {
            assert_eq!(
                interpret(said, false),
                Heard::Nothing,
                "{said:?} should not have woken him"
            );
        }
    }

    /// A bare name is not a wake word. Saying "Carl" while talking about someone else must
    /// not start recording.
    #[test]
    fn the_name_alone_is_not_enough() {
        assert_eq!(interpret("tell Carl I said hello", false), Heard::Nothing);
    }

    #[test]
    fn once_listening_everything_is_for_him() {
        assert_eq!(
            interpret("what should I research next", true),
            Heard::Say("what should i research next".into())
        );
    }

    #[test]
    fn the_end_phrase_ends_it() {
        for said in ["end conversation", "OK, end conversation.", "bye Carl"] {
            assert_eq!(interpret(said, true), Heard::End, "{said:?} should end it");
        }
    }

    /// Whisper writes "and" for "end" often enough that the strict spelling would strand
    /// someone mid conversation with no way out.
    #[test]
    fn a_misheard_end_still_ends_it() {
        assert_eq!(interpret("and conversation", true), Heard::End);
        assert_eq!(interpret("end convo", true), Heard::End);
    }

    /// The end phrase only means the end while he is listening. Said in passing when he is
    /// idle it is just words, and must not be treated as a command.
    #[test]
    fn the_end_phrase_is_ignored_when_he_is_not_listening() {
        assert_eq!(interpret("end conversation", false), Heard::Nothing);
    }

    #[test]
    fn waking_does_not_need_the_end_check() {
        // "Hey Carl, end conversation" wakes him. Ending immediately is the caller's problem
        // and not something this function should guess at.
        assert!(matches!(
            interpret("hey carl end conversation", false),
            Heard::Wake { .. }
        ));
    }
}
