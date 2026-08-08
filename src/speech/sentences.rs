//! Pulling speakable sentences out of an answer that is still being written.
//!
//! `speakable` works on a finished answer, where it can see every code fence open and close.
//! A stream cannot. Text arrives a few words at a time, a fence opens in one piece and closes
//! four seconds later, and deciding what to say has to happen before either is known.
//!
//! So this holds the raw text and only lets go of a piece once it is certain what that piece
//! is. Certainty comes from a newline for anything that might be markup, and from a full stop
//! for ordinary prose mid paragraph. Saying a sentence early is the entire point, and saying
//! the contents of a code block out loud is unbearable, so the two rules have to coexist.

use super::speakable;

#[derive(Default)]
pub struct Sentences {
    /// Raw text not yet handed out.
    buf: String,
    /// Inside a fenced code block, where nothing is read aloud.
    in_code: bool,
}

impl Sentences {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, text: &str) {
        self.buf.push_str(text);
    }

    /// The next thing worth saying, if there is one yet.
    ///
    /// Returns `None` while the buffer holds only a partial line that might turn out to be a
    /// fence, a heading or a list marker. Guessing early and being wrong means reading a
    /// backtick out loud.
    pub fn take(&mut self) -> Option<String> {
        loop {
            let piece = self.next_piece()?;
            let said = speakable(&piece);
            if !said.is_empty() {
                return Some(said);
            }
            // A blank line, a bullet marker on its own, or a line inside a code block. Keep
            // going rather than returning an empty string that would start a silent player.
        }
    }

    /// Everything left over, once the answer is complete.
    pub fn rest(&mut self) -> Option<String> {
        let tail = std::mem::take(&mut self.buf);
        self.in_code = false;
        let said = speakable(&tail);
        (!said.is_empty()).then_some(said)
    }

    /// One complete sentence, or one complete line when the line is markup.
    ///
    /// A sentence inside a line is taken before the line ends. Waiting for the newline would
    /// make the split depend on where a paragraph happens to wrap, so the same answer would
    /// be spoken in different pieces depending on how it arrived.
    fn next_piece(&mut self) -> Option<String> {
        if self.in_code {
            let nl = self.buf.find('\n')?;
            let line: String = self.buf.drain(..=nl).collect();
            if line.trim_start().starts_with("```") {
                self.in_code = false;
            }
            // Nothing inside a code block is ever said, including the fence that closes it.
            return Some(String::new());
        }

        let nl = self.buf.find('\n');
        let line = &self.buf[..nl.unwrap_or(self.buf.len())];
        let trimmed = line.trim_start();

        if trimmed.starts_with("```") {
            // The fence is only certainly a fence once the line is whole.
            let end = nl? + 1;
            self.buf.drain(..end);
            self.in_code = true;
            // Only the opening fence is announced. Saying it again on the way out would
            // claim there were two code blocks.
            return Some("```\n".to_string());
        }

        // Headings and bullets are decided by their first characters, which arrive before the
        // rest of the line. Emitting early would read the hash or the dash out loud.
        if trimmed.is_empty() || trimmed.starts_with(['`', '#', '-', '*', '>', '+']) {
            let end = nl? + 1;
            return Some(self.buf.drain(..end).collect());
        }

        if let Some(end) = sentence_end(line) {
            return Some(self.buf.drain(..end).collect());
        }
        let end = nl? + 1;
        Some(self.buf.drain(..end).collect())
    }
}

/// Where the first complete sentence ends, if one has finished.
///
/// A full stop alone is not enough. "1.5" and "e.g." and "No. 4" all contain one, and a
/// sentence that ends mid number is read out as two.
fn sentence_end(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    for (i, c) in s.char_indices() {
        if !matches!(c, '.' | '!' | '?') {
            continue;
        }
        // Something has to follow, or the sentence may simply not be finished being typed.
        let after = s[i + c.len_utf8()..].chars().next()?;
        if !after.is_whitespace() {
            continue;
        }
        // A digit either side of a full stop is a number, not the end of a thought.
        if c == '.' && i > 0 && b[i - 1].is_ascii_digit() {
            continue;
        }
        // Ellipsis and abbreviations. One letter before a full stop is nearly always an
        // initial or a shorthand rather than a word.
        if c == '.' && i > 0 && (b[i - 1] == b'.' || is_one_letter_word(&s[..i])) {
            continue;
        }
        return Some(i + c.len_utf8() + after.len_utf8());
    }
    None
}

fn is_one_letter_word(before: &str) -> bool {
    let mut chars = before.chars().rev();
    match (chars.next(), chars.next()) {
        (Some(a), Some(b)) => a.is_alphabetic() && !b.is_alphanumeric(),
        (Some(a), None) => a.is_alphabetic(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drain(s: &mut Sentences) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(p) = s.take() {
            out.push(p);
        }
        out
    }

    /// The whole point. A sentence is spoken before the rest of the answer exists.
    #[test]
    fn a_finished_sentence_comes_out_before_the_next_one_arrives() {
        let mut s = Sentences::new();
        s.feed("Research automation science. Then bui");
        assert_eq!(drain(&mut s), vec!["Research automation science."]);

        s.feed("ld a second furnace.");
        assert_eq!(
            s.take(),
            None,
            "no trailing space yet, so it may not be done"
        );

        s.feed(" ");
        assert_eq!(drain(&mut s), vec!["Then build a second furnace."]);
    }

    #[test]
    fn a_half_written_sentence_is_held_back() {
        let mut s = Sentences::new();
        s.feed("Research automa");
        assert_eq!(drain(&mut s), Vec::<String>::new());
        assert_eq!(s.rest().as_deref(), Some("Research automa"));
    }

    /// A code fence arrives in pieces and closes seconds later. Reading its contents aloud is
    /// unbearable, and the decision has to be made before the closing fence exists.
    #[test]
    fn a_code_block_is_announced_once_and_never_recited() {
        let mut s = Sentences::new();
        s.feed("Try this:\n```rust\nlet x = 1;\n");
        let said = drain(&mut s);
        assert_eq!(said.len(), 2, "{said:?}");
        assert_eq!(said[0], "Try this:");
        assert!(said[1].contains("code block on screen"), "{said:?}");

        s.feed("let y = 2;\n```\nThat should work.\n");
        let said = drain(&mut s);
        assert_eq!(
            said,
            vec!["That should work."],
            "the code must not be read, and the closing fence must not be announced"
        );
    }

    /// The failure that would be obvious out loud: a decimal read as two sentences.
    #[test]
    fn a_number_is_not_a_sentence_ending() {
        let mut s = Sentences::new();
        s.feed("You need 1.5 stacks of iron plate. ");
        assert_eq!(drain(&mut s), vec!["You need 1.5 stacks of iron plate."]);
    }

    #[test]
    fn an_abbreviation_does_not_end_a_sentence() {
        let mut s = Sentences::new();
        s.feed("Ask J. R. about it later. ");
        assert_eq!(drain(&mut s), vec!["Ask J. R. about it later."]);
    }

    #[test]
    fn questions_and_exclamations_end_sentences_too() {
        let mut s = Sentences::new();
        s.feed("Do you have coal? Yes! Good. ");
        assert_eq!(
            drain(&mut s),
            vec!["Do you have coal?", "Yes!", "Good."],
            "each should be spoken as it lands"
        );
    }

    /// A heading is decided by its first character, which arrives before the rest of the
    /// line. Emitting early would read the hash out loud.
    #[test]
    fn markup_waits_for_the_end_of_its_line() {
        let mut s = Sentences::new();
        s.feed("## Next steps. Still the heading");
        assert_eq!(drain(&mut s), Vec::<String>::new(), "must wait");

        s.feed("\n- Get iron\n");
        assert_eq!(
            drain(&mut s),
            vec!["Next steps. Still the heading", "Get iron"]
        );
    }

    /// Nothing worth saying must not produce an empty string. An empty string starts a
    /// player process that plays silence and then exits, which reads as a stutter.
    #[test]
    fn blank_lines_produce_no_speech_at_all() {
        let mut s = Sentences::new();
        s.feed("\n\n   \n\nReal words here.\n");
        assert_eq!(drain(&mut s), vec!["Real words here."]);
        assert_eq!(s.rest(), None);
    }

    /// Whatever is left when the answer ends still gets said, even without a full stop.
    #[test]
    fn the_last_words_are_not_lost_for_want_of_a_full_stop() {
        let mut s = Sentences::new();
        s.feed("First one. And then a trailing thought with no stop");
        assert_eq!(drain(&mut s), vec!["First one."]);
        assert_eq!(
            s.rest().as_deref(),
            Some("And then a trailing thought with no stop")
        );
        assert_eq!(s.rest(), None, "and it is only said once");
    }

    /// Fed one character at a time, the result must be identical. Chunk boundaries from the
    /// network are arbitrary and must not change a single word.
    #[test]
    fn the_split_between_chunks_changes_nothing() {
        let whole = "Get iron first. Then coal.\n```\nx\n```\nDone.\n";

        let mut a = Sentences::new();
        a.feed(whole);
        let at_once = drain(&mut a);

        let mut b = Sentences::new();
        let mut piecemeal = Vec::new();
        for c in whole.chars() {
            b.feed(&c.to_string());
            while let Some(p) = b.take() {
                piecemeal.push(p);
            }
        }
        assert_eq!(at_once, piecemeal);
    }
}
