//! What the shared pieces have to keep being true about themselves.

use super::*;

/// The two states nobody has to act on must not carry an alarming colour, and the two that
/// do must not be the same as each other.
#[test]
fn colour_means_state_and_nothing_else() {
    assert_eq!(status_color(AgentStatus::Unknown), theme::UNKNOWN);
    assert_eq!(status_color(AgentStatus::Blocked), theme::BAD);
    assert_ne!(
        status_color(AgentStatus::Working),
        status_color(AgentStatus::Idle),
        "busy and idle must be distinguishable at a glance"
    );
    assert_eq!(health_color(Health::Unknown), theme::UNKNOWN);
    assert_eq!(health_color(Health::Failed), theme::BAD);
    assert!(!wants_attention(Health::Unknown), "a gap is not an alarm");
    assert!(wants_attention(Health::Failed));
}

/// The honesty rule that outlives the redesign: a state is never carried by colour alone.
///
/// Degraded and blocked are both amber, which is correct, so they have to be told apart by
/// their shape. Every health and every status therefore gets its own mark, and no two states
/// within one set may share one.
#[test]
fn every_state_has_its_own_shape_as_well_as_its_colour() {
    let healths = [
        Health::Healthy,
        Health::Degraded,
        Health::Blocked,
        Health::Failed,
        Health::Unknown,
    ];
    let mut marks: Vec<&str> = healths.iter().map(|h| health_mark(*h).name()).collect();
    let before = marks.len();
    marks.sort_unstable();
    marks.dedup();
    assert_eq!(marks.len(), before, "two healths share a shape");

    assert_eq!(
        health_color(Health::Degraded),
        health_color(Health::Blocked),
        "these two deliberately share a colour, which is why the shape has to differ"
    );
    assert_ne!(health_mark(Health::Degraded), health_mark(Health::Blocked));

    let statuses = [
        AgentStatus::Working,
        AgentStatus::AwaitingReview,
        AgentStatus::Blocked,
        AgentStatus::Idle,
        AgentStatus::Unknown,
    ];
    let mut marks: Vec<&str> = statuses.iter().map(|s| status_mark(*s).name()).collect();
    let before = marks.len();
    marks.sort_unstable();
    marks.dedup();
    assert_eq!(marks.len(), before, "two statuses share a shape");
}

/// Unknown is a gap, so it is drawn as one. A box, filled or not, reads as a state somebody
/// chose; a bare line reads as nothing having been said.
#[test]
fn unknown_is_drawn_as_an_absence_rather_than_as_a_state() {
    assert_eq!(health_mark(Health::Unknown), Mark::Dash);
    assert_eq!(status_mark(AgentStatus::Unknown), Mark::Dash);
    for h in [Health::Healthy, Health::Degraded, Health::Failed] {
        assert_ne!(health_mark(h), Mark::Dash);
    }
}

#[test]
fn ago_says_the_shortest_true_thing() {
    assert_eq!(ago(100, 100), "now");
    assert_eq!(ago(130, 100), "30s ago");
    assert_eq!(ago(1000, 100), "15m ago");
    assert_eq!(ago(100_000, 100), "1d ago");
}

/// A state does not decay, so it is not given an age. A sample does, and one that was never
/// taken says exactly that rather than showing a plausible number.
#[test]
fn only_a_sample_is_described_as_fresh_or_stale() {
    let state = Diagnostic::new("army.tasks", Health::Healthy, "x", Kind::EventDriven);
    assert_eq!(freshness(&state, 100), None);

    let never = Diagnostic::new("system.gpu", Health::Unknown, "x", Kind::Sampled);
    assert_eq!(freshness(&never, 100).as_deref(), Some("never sampled"));

    let taken = Diagnostic::new("system.cpu", Health::Healthy, "x", Kind::Sampled).measured(70);
    assert_eq!(freshness(&taken, 100).as_deref(), Some("sampled 30s ago"));
}

/// JJ is not one of the agents, and the card that carries him must not be able to be drawn
/// in the same tone as one.
#[test]
fn the_authority_tone_is_not_the_ordinary_one() {
    assert_ne!(Tone::Authority, Tone::default());
    assert_eq!(Tone::default(), Tone::Normal);
}
