//! Working out whether a message that says "Carl" is actually talking to Carl.
//!
//! A mention is unambiguous. A name is not. These are all the word Carl and only some of them
//! want an answer:
//!
//! ```text
//!   carl what should I research next     to him
//!   what do you think, Carl?             to him
//!   I asked Carl yesterday               about him
//!   Carl's memory design is good         about him
//!   ask carl when he is back             about him, and to somebody else
//! ```
//!
//! Answering the bottom three is worse than missing the top two. Missing one means JJ says it
//! again with an at sign. Answering one means Carl butting into a conversation between two
//! people who were discussing him, which is the kind of thing that gets a bot thrown out of a
//! channel.
//!
//! So the rule is conservative on purpose: the name has to be at the very start or the very
//! end. That is where a name goes when you are speaking to somebody, and almost never where
//! it goes when you are speaking about them.
//!
//! Pure text in, a decision out. No Slack and no model.

/// Spellings that count as his name. Exact, unlike the voice list, because typing is not
/// mishearing. `heard.rs` accepts "call" and "carol" because whisper produces those from
/// clean speech, and a person typing does not.
pub const NAMES: [&str; 2] = ["carl", "karl"];

/// Words that can sit in front of a name without changing who is being spoken to.
const GREETINGS: [&str; 8] = ["hey", "hi", "hello", "ok", "okay", "yo", "so", "um"];

/// Whether this message is addressed to Carl, and what is left once his name is taken out.
///
/// `None` means it is not for him, including when it mentions him. Being talked about is not
/// being talked to.
pub fn addressed(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return None;
    }

    // Possessive is always about him. "Carl's design" is never a question for Carl.
    if words.iter().any(|w| {
        let l = w.to_lowercase();
        NAMES.iter().any(|n| l.starts_with(&format!("{n}'s")))
    }) {
        return None;
    }

    // A range rather than one word, because a greeting in front of the name is only there to
    // introduce it. "hey carl can you check this" is asking "can you check this", and leaving
    // the "hey" in would send Claude a question that starts with a stray greeting.
    let cut = address(&words)?;
    let rest: Vec<&str> = words
        .iter()
        .enumerate()
        .filter(|(i, _)| !cut.contains(i))
        .map(|(_, w)| *w)
        .collect();

    let left = rest.join(" ");
    let left = left
        .trim()
        .trim_start_matches([',', ':', '-', ' '])
        .trim()
        .to_string();

    // A name and nothing else. Being called is not being asked anything, but it is being
    // spoken to, so it gets a reply rather than silence.
    if left.is_empty() {
        return Some(String::new());
    }
    Some(left)
}

/// Which words are the address itself, if this is one.
fn address(words: &[&str]) -> Option<std::ops::Range<usize>> {
    let is_name = |w: &str| {
        let l: String = w
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        NAMES.contains(&l.as_str())
    };

    // At the front, possibly after a greeting. "hey carl", "carl, look at this".
    for (i, w) in words.iter().enumerate().take(3) {
        if is_name(w) {
            let only_greetings = words[..i]
                .iter()
                .all(|p| GREETINGS.contains(&trim_word(p).as_str()));
            if only_greetings {
                // The greeting goes with the name it introduced.
                return Some(0..i + 1);
            }
            break;
        }
    }

    // At the very end, which is where a name goes when you turn to somebody. "what do you
    // think, carl?" The last word may carry punctuation.
    let last = words.len() - 1;
    if is_name(words[last]) && words.len() > 1 {
        return Some(last..last + 1);
    }

    None
}

fn trim_word(w: &str) -> String {
    w.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn his_name_at_the_front_is_an_address() {
        assert_eq!(
            addressed("carl what should I research next").as_deref(),
            Some("what should I research next")
        );
        assert_eq!(
            addressed("Carl, look at the smelters").as_deref(),
            Some("look at the smelters")
        );
        assert_eq!(
            addressed("hey Carl can you check this").as_deref(),
            Some("can you check this"),
            "the greeting only introduced the name, so it goes too"
        );
        assert_eq!(
            addressed("CARL HELP").as_deref(),
            Some("HELP"),
            "shouting is still addressing"
        );
    }

    /// Turning to somebody at the end of a sentence is the other place a name means you.
    #[test]
    fn his_name_at_the_end_is_an_address() {
        assert_eq!(
            addressed("what do you think, Carl?").as_deref(),
            Some("what do you think,")
        );
        assert_eq!(addressed("any ideas karl").as_deref(), Some("any ideas"));
    }

    /// The important half. Butting into a conversation about him is worse than missing one
    /// aimed at him, because missing one costs an at sign and butting in costs the channel.
    #[test]
    fn being_talked_about_is_not_being_talked_to() {
        for about in [
            "I asked Carl yesterday and he said no",
            "ask carl when he gets back",
            "Carl's memory design is the good bit",
            "the thing carl built last week works",
            "tell Carl about the smelters when you see him",
            "I think Carl would know the answer to that",
        ] {
            assert_eq!(addressed(about), None, "should have stayed out of: {about}");
        }
    }

    #[test]
    fn a_message_with_no_name_in_it_is_not_for_him() {
        assert_eq!(addressed("what should we research next"), None);
        assert_eq!(addressed(""), None);
        assert_eq!(addressed("carlos is coming over"), None);
        assert_eq!(addressed("we need more carlite"), None);
    }

    /// Just his name is being called rather than being asked, and a person who calls a name
    /// expects an answer.
    #[test]
    fn being_called_by_name_alone_still_gets_a_reply() {
        assert_eq!(addressed("Carl").as_deref(), Some(""));
        assert_eq!(addressed("carl?").as_deref(), Some(""));
        assert_eq!(addressed("hey carl").as_deref(), Some(""));
    }

    /// Punctuation attached to the name must not hide it.
    #[test]
    fn punctuation_does_not_hide_the_name() {
        assert!(addressed("carl! the belt is backed up").is_some());
        assert!(addressed("(carl) have a look").is_some());
    }
}
