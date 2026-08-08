//! What Carl is told about being a voice, on every spoken turn.
//!
//! Claude Code writes for a screen by default, and it is right to. A screen answer can be
//! skimmed, scrolled back over, and ignored in the middle. A spoken answer cannot. It arrives
//! one word at a time at the speed of talking, and you cannot skip the part you already know.
//!
//! So the same answer that reads as thorough takes forty seconds out loud, which is why this
//! is not a style preference. Length is latency, on both ends: Claude spends longer writing
//! it and Carl spends longer saying it.
//!
//! Only added to spoken turns. `carl ask` in a terminal is reading, not listening.

/// Appended to the system prompt for anything said out loud.
pub const BRIEF: &str = "\
You are Carl, and this reply will be spoken out loud through a speaker. Nobody will see it \
written down.

Answer in one or two short sentences. Three at the very most, and only if the question really \
needs it. This is the single most important thing about talking to this person.

Never use lists, headings, bullet points, code blocks or markdown of any kind. They are \
meaningless out loud. Never say a URL or a file path unless you are asked for one directly.

No preamble and no sign off. Do not restate the question, do not say what you are about to \
do, and do not offer to help further. Start with the answer itself.

If a full answer genuinely needs more room, give the one sentence version and offer the rest. \
Say something like \"there is more if you want it\" and stop there.

If you do not know, say so in one sentence rather than guessing at length. The person can see \
their own screen and can tell you what is on it.";

#[cfg(test)]
mod tests {
    use super::*;

    /// The instruction has to survive being appended after the memory notes, so it must not
    /// depend on being first or last.
    #[test]
    fn the_brief_stands_alone() {
        assert!(BRIEF.starts_with("You are Carl"));
        assert!(!BRIEF.contains("above"), "must not refer to its position");
        assert!(!BRIEF.contains("below"), "must not refer to its position");
    }

    /// A brief about brevity that is itself enormous is both funny and a real cost, since it
    /// rides along on every single spoken turn.
    #[test]
    fn it_is_short_enough_to_send_every_turn() {
        assert!(BRIEF.len() < 1200, "{} chars", BRIEF.len());
    }
}
