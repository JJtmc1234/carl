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
        }
    }

    /// JJ saw a lone square under Carl's name and asked what it was.
    ///
    /// It was `\u{2588}`, a full block, appended as a cursor. Against text it reads as a fat
    /// caret. Against Carl's opening empty turn it is a brick with no explanation.
    #[test]
    fn an_answer_that_has_not_started_says_it_is_thinking() {
        let out = text_of(&turn("", true), 0.0);
        assert!(out.starts_with("thinking"), "{out:?}");
        assert!(
            !out.contains('\u{2588}'),
            "the block is still there: {out:?}"
        );
    }

    /// A still indicator cannot be told from a stuck one.
    #[test]
    fn the_thinking_state_moves() {
        let a = text_of(&turn("", true), 0.0);
        let b = text_of(&turn("", true), 0.6);
        let c = text_of(&turn("", true), 1.1);
        assert!(a != b || b != c, "it never changes: {a:?} {b:?} {c:?}");
    }

    /// Once words are arriving the cursor is a thin bar, not a brick.
    #[test]
    fn an_arriving_answer_gets_a_thin_cursor() {
        let out = text_of(&turn("belt rate is", true), 0.0);
        assert!(out.starts_with("belt rate is"));
        assert!(out.ends_with('\u{258f}'), "{out:?}");
        assert!(!out.contains('\u{2588}'));
    }

    /// A finished answer has no cursor at all.
    #[test]
    fn a_finished_answer_carries_no_marker() {
        assert_eq!(text_of(&turn("done", false), 0.0), "done");
    }
}
