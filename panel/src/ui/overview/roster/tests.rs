//! The order the army block is read in.

use super::*;
use crate::source::{MockPanelDataSource, PanelDataSource};

fn agent(name: &str, status: AgentStatus) -> AgentView {
    let mut v = AgentView::unknown(name);
    v.status = status;
    v
}

/// Worst first. A screen sorted by name buries the one agent that has stopped, which is the
/// only one on the list somebody has to do something about.
#[test]
fn the_block_is_ordered_by_how_much_somebody_should_care() {
    let agents = vec![
        agent("adrian", AgentStatus::Idle),
        agent("carl", AgentStatus::Working),
        agent("mason", AgentStatus::Unknown),
        agent("nora", AgentStatus::Blocked),
        agent("pip", AgentStatus::AwaitingReview),
    ];
    let names: Vec<&str> = on_deck(&agents).iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["nora", "pip", "carl", "adrian", "mason"]);
}

/// Ties keep the order they arrived in, so nothing swaps places under the pointer between one
/// frame and the next.
#[test]
fn agents_in_the_same_state_keep_a_stable_order() {
    let agents = vec![
        agent("zed", AgentStatus::Idle),
        agent("abe", AgentStatus::Idle),
        agent("mid", AgentStatus::Idle),
    ];
    let names: Vec<&str> = on_deck(&agents).iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["zed", "abe", "mid"],
        "arrival order, not alphabetical"
    );
}

/// JJ is not an agent and must not appear in a block headed "the army".
#[test]
fn the_person_is_not_in_the_army_block() {
    let s = MockPanelDataSource::new().snapshot();
    let names: Vec<&str> = on_deck(&s.agents).iter().map(|a| a.name.as_str()).collect();
    assert!(!names.contains(&"jj"), "{names:?}");
    assert_eq!(names.len(), s.agents.len() - 1);
}

/// The cap has to be big enough that the real organisation fits without a "more" link, and
/// small enough that a big one does not push the feed off the screen.
#[test]
fn the_cap_fits_the_organisation_as_it_stands() {
    let s = MockPanelDataSource::new().snapshot();
    assert!(
        on_deck(&s.agents).len() <= SHOWN,
        "the real chain should not need the overflow link yet"
    );
    // Compared through a variable so the compiler cannot fold it away. The assertion is the
    // point: whoever raises SHOWN has to come here and decide whether a summary is still one.
    let shown = SHOWN;
    assert!(shown < 12, "a block of twelve is a screen, not a summary");
}
