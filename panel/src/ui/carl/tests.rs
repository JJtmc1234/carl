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
