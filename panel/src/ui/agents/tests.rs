//! The wording decisions this screen makes, checked without painting.

use super::*;

fn view(name: &str) -> AgentView {
    AgentView::unknown(name)
}

/// The role line has to say something for everybody. An agent with no department recorded
/// still has a rank, and one with neither says so rather than leaving a gap that reads as a
/// rendering fault.
#[test]
fn every_agent_gets_a_role_line() {
    let mut v = view("nora");
    assert_eq!(
        role_of(&v),
        "worker",
        "the rank carries it when nothing else does"
    );

    v.department = Some("coding".into());
    assert_eq!(role_of(&v), "coding");

    v.sub_department = Some("factorio".into());
    assert_eq!(
        role_of(&v),
        "coding / factorio",
        "both are shown, because a sub department alone loses which department it is in"
    );

    let stranger = view("nobody-in-the-org");
    assert_eq!(role_of(&stranger), "role not recorded");
}

/// A blocker outranks an activity, because the reason somebody is looking at the card is that
/// it stopped. And an agent nothing is known about says exactly that.
#[test]
fn the_work_line_says_the_most_important_true_thing() {
    let mut v = view("nora");
    let (text, colour) = work_line(&v);
    assert_eq!(text, "nothing has reported on this agent");
    assert_eq!(colour, theme::UNKNOWN, "a gap is drawn as a gap");

    v.status = AgentStatus::Idle;
    v.last_activity = Some("finished the smelting ratio task".into());
    let (text, colour) = work_line(&v);
    assert_eq!(text, "finished the smelting ratio task");
    assert_eq!(colour, theme::DIM);

    v.status = AgentStatus::Blocked;
    v.blocker = Some("run-tests.sh needs python3-pytest".into());
    let (text, colour) = work_line(&v);
    assert_eq!(text, "run-tests.sh needs python3-pytest");
    assert_eq!(
        colour,
        theme::BAD,
        "a blocker is the one line that is allowed to be loud"
    );
}

/// Folding everything must fold every manager and nobody else, and it must never fold JJ,
/// who is not in the tree to be folded.
#[test]
fn folding_everything_folds_the_managers_and_not_the_person() {
    let all = every_manager();
    assert!(all.contains("carl"));
    assert!(all.contains("adrian"));
    assert!(all.contains("mason"));
    assert!(!all.contains("nora"), "a worker has nothing to fold");
    assert!(!all.contains("jj"), "the person is not part of the tree");
}
