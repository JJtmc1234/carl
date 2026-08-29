//! The edge rules, tested without starting a `claude` process.
//!
//! `hand` runs a real agent, so these cover the part that decides whether it should run at all.
//! That is the part JJ reported as broken, and it is the part a wrong answer is expensive in.

use crate::army::org;

/// The shape of the bug. Carl reaching Miles directly is what the subagent tool let him fake,
/// and it has to be refused by the same rule whatever route is used to ask for it.
#[test]
fn carl_cannot_hand_straight_to_a_worker() {
    assert!(!org::may_delegate("carl", "miles"));
    assert!(!org::may_delegate("carl", "nora"));
    assert!(!org::may_delegate("carl", "iris"));
    assert!(!org::may_delegate("carl", "evan"));
}

/// The route that should be taken instead, and the one the refusal names.
#[test]
fn carl_hands_to_leads_and_leads_hand_to_their_own() {
    for lead in ["adrian", "mason", "olivia", "serena", "rowan"] {
        assert!(org::may_delegate("carl", lead), "carl cannot reach {lead}");
    }
    assert!(org::may_delegate("olivia", "miles"));
    assert!(org::may_delegate("adrian", "iris"));
    assert!(org::may_delegate("adrian", "evan"));
    assert!(org::may_delegate("mason", "nora"));
}

/// A refusal has to say what would have worked. "No" on its own gets retried or worked around,
/// which is how the subagent shortcut got invented in the first place.
#[test]
fn a_refusal_names_the_lead_to_go_through() {
    let why = org::check_delegation("carl", "miles")
        .expect_err("carl must not reach miles")
        .to_string();
    assert!(why.contains("miles"), "{why}");
    assert!(why.contains("olivia"), "the refusal must name the route: {why}");
}

/// Nobody hands work upwards, and nobody hands work sideways.
#[test]
fn work_never_goes_up_or_sideways() {
    assert!(!org::may_delegate("miles", "olivia"), "upwards");
    assert!(!org::may_delegate("olivia", "carl"), "upwards");
    assert!(!org::may_delegate("olivia", "adrian"), "sideways between leads");
    assert!(!org::may_delegate("miles", "nora"), "sideways between workers");
}

/// A worker has nobody below it, so a worker that tries to delegate is refused rather than
/// quietly starting something.
#[test]
fn a_worker_can_hand_to_nobody() {
    for worker in ["miles", "nora", "iris", "evan"] {
        for anyone in ["carl", "olivia", "adrian", "mason", "miles", "nora"] {
            assert!(
                !org::may_delegate(worker, anyone),
                "{worker} must not be able to hand to {anyone}"
            );
        }
    }
}

/// An agent cannot hand work to itself, which would be a loop that looks like progress.
#[test]
fn nobody_hands_work_to_themselves() {
    for who in ["carl", "olivia", "adrian", "mason", "miles", "nora"] {
        assert!(!org::may_delegate(who, who), "{who} handed to itself");
    }
}
