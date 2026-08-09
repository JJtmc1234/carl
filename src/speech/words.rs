//! Turning what Claude wrote into something worth hearing out loud.
//!
//! Claude writes for a screen. Code fences, bullet markers and bare URLs are all fine to read
//! and dismal to listen to. This is pure text in and text out, so every rule about what Carl
//! says aloud is testable without a speaker.

/// Turns an answer into something worth hearing out loud.
///
/// Claude writes for a screen. Code fences, bullet markers and bare URLs are all fine to read
/// and dismal to listen to, and a spoken reply that recites a URL character by character is
/// worse than one that skips it.
pub fn speakable(raw: &str) -> String {
    let mut out = Vec::new();
    let mut in_code = false;

    for line in raw.lines() {
        let t = line.trim();

        if t.starts_with("```") {
            in_code = !in_code;
            if in_code {
                out.push("There is a code block on screen.".to_string());
            }
            continue;
        }
        if in_code || t.is_empty() {
            continue;
        }
        // Never said out loud. This is Carl writing something down, not talking.
        if crate::remember::is_note(t) {
            continue;
        }

        // Strip leading list markers and heading hashes. "Dash dash dash" is not speech.
        let t = t
            .trim_start_matches(['#', '-', '*', '>', '+'])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')'])
            .trim();

        if t.is_empty() {
            continue;
        }
        // Inline emphasis and code ticks are punctuation to the eye and noise to the ear.
        out.push(t.replace(['*', '_', '`'], ""));
    }

    out.join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prose_is_left_alone() {
        assert_eq!(
            speakable("Research automation science, then build a second furnace."),
            "Research automation science, then build a second furnace."
        );
    }

    #[test]
    fn list_markers_and_headings_are_not_read_out() {
        assert_eq!(
            speakable("## Next steps\n\n- Get iron\n- Then copper\n1. Finally coal"),
            "Next steps Get iron Then copper Finally coal"
        );
    }

    /// Reading a code block aloud is unbearable, and skipping it silently is confusing, so
    /// Carl says there is one.
    #[test]
    fn code_blocks_are_mentioned_rather_than_recited() {
        let said = speakable("Try this:\n```rust\nlet x = 1;\nlet y = 2;\n```\nThat should work.");
        assert!(said.contains("code block on screen"), "{said}");
        assert!(!said.contains("let x"), "{said}");
        assert!(said.contains("That should work"), "{said}");
    }

    #[test]
    fn inline_markup_is_stripped() {
        assert_eq!(
            speakable("Use the **assembling machine**, not the `furnace`."),
            "Use the assembling machine, not the furnace."
        );
    }

    /// Saying nothing is better than making the speakers pop for an empty string.
    #[test]
    fn nothing_worth_saying_comes_back_empty() {
        assert_eq!(speakable(""), "");
        assert_eq!(
            speakable("```\ncode only\n```"),
            "There is a code block on screen."
        );
    }
}
