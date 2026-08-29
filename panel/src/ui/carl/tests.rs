//! What the console has to keep being true about itself.

use super::composer::Act;
use crate::app::App;
use crate::source::MockPanelDataSource;

fn app() -> App {
    App::new(Box::new(MockPanelDataSource::new()))
}

/// Saying something and setting an objective are different acts with different consequences,
/// and the composer has to say which is which. A control whose two modes describe themselves
/// the same way is a control that sends the wrong command.
#[test]
fn the_two_acts_describe_themselves_differently() {
    let acts = [Act::Say, Act::Objective];
    let labels: Vec<&str> = acts.iter().map(|a| a.label()).collect();
    let hints: Vec<&str> = acts.iter().map(|a| a.hint()).collect();
    let consequences: Vec<&str> = acts.iter().map(|a| a.consequence()).collect();

    assert_ne!(labels[0], labels[1]);
    assert_ne!(hints[0], hints[1]);
    assert_ne!(consequences[0], consequences[1]);
    for text in consequences {
        assert!(text.contains("Carl"), "{text} does not say where it goes");
    }
}

/// The two buffers stay separate. One box on screen must not mean one buffer underneath, or
/// a half typed objective would be sent as a message the moment the mode changed.
#[test]
fn the_two_acts_keep_separate_buffers() {
    let mut a = app();
    a.draft = "what is nora doing".into();
    a.objective = "correct the smelting ratios".into();

    a.send_draft();
    assert!(a.draft.is_empty(), "the message went");
    assert_eq!(
        a.objective, "correct the smelting ratios",
        "and took nothing else with it"
    );
}

#[cfg(test)]
mod thinking_tests {
    use crate::model::{Speaker, Turn};
    use crate::ui::carl::conversation::text_of;

    fn turn(text: &str, streaming: bool) -> Turn {
        Turn {
            at: 0,
            from: Speaker::Carl,
            text: text.into(),
            streaming,
            thinking: String::new(),
            doing: Vec::new(),
        }
    }

    /// JJ saw a lone square under Carl's name, twice, and asked what it was.
    ///
    /// First it was `\u{2588}` and then `\u{258f}`, and swapping one for the other changed
    /// nothing, because the square was never the character. Ubuntu-Light is the proportional
    /// face this text is drawn in and it contains no Block Elements at all, so egui drew its
    /// missing glyph box both times. Only ASCII is safe here.
    #[test]
    fn an_answer_that_has_not_started_says_it_is_working() {
        let out = text_of(&turn("", true), 0.0);
        assert!(out.starts_with("working"), "{out:?}");
    }

    /// A still indicator cannot be told from a stuck one.
    #[test]
    fn the_thinking_state_moves() {
        let a = text_of(&turn("", true), 0.0);
        let b = text_of(&turn("", true), 0.6);
        let c = text_of(&turn("", true), 1.1);
        assert!(a != b || b != c, "it never changes: {a:?} {b:?} {c:?}");
    }

    /// Once words are arriving the cursor is an ASCII bar.
    #[test]
    fn an_arriving_answer_gets_an_ascii_cursor() {
        let out = text_of(&turn("belt rate is", true), 0.0);
        assert!(out.starts_with("belt rate is"));
        assert!(out.ends_with('|'), "{out:?}");
    }

    /// The real bug, stated as a rule. Any glyph outside ASCII is a gamble on a font that has
    /// already been lost twice, so nothing here may reach for one.
    #[test]
    fn nothing_the_conversation_draws_leaves_ascii() {
        for text in ["", "belt rate is", "done"] {
            for streaming in [true, false] {
                for tick in [0.0, 0.4, 0.9, 1.6] {
                    let out = text_of(&turn(text, streaming), tick);
                    let stray: Vec<char> = out.chars().filter(|c| !c.is_ascii()).collect();
                    assert!(stray.is_empty(), "non ascii {stray:?} in {out:?}");
                }
            }
        }
    }

    /// The blink must not move the text sideways. An off frame that is simply shorter makes
    /// the whole paragraph twitch twice a second.
    #[test]
    fn the_blink_keeps_the_width() {
        let on = text_of(&turn("steady", true), 0.0);
        let off = text_of(&turn("steady", true), 0.6);
        assert_ne!(on, off, "it never blinks");
        assert_eq!(on.chars().count(), off.chars().count(), "{on:?} {off:?}");
    }

    /// A finished answer has no cursor at all.
    #[test]
    fn a_finished_answer_carries_no_marker() {
        assert_eq!(text_of(&turn("done", false), 0.0), "done");
    }
}
