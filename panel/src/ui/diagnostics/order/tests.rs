//! The ordering and grouping rules, checked without a window.

use super::*;
use crate::model::Kind;

fn d(component: &str, health: Health) -> Diagnostic {
    Diagnostic::new(component, health, "", Kind::EventDriven)
}

/// The screen must order itself by what needs somebody, and stably, or rows swap places
/// under the pointer every frame.
#[test]
fn the_worst_sorts_to_the_top_and_ties_are_stable() {
    let all = vec![
        d("army.b-healthy", Health::Healthy),
        d("army.a-unknown", Health::Unknown),
        d("army.c-failed", Health::Failed),
        d("army.a-degraded", Health::Degraded),
        d("army.b-degraded", Health::Degraded),
    ];
    let order: Vec<&str> = sorted(&all, "army")
        .iter()
        .map(|d| d.component.as_str())
        .collect();
    assert_eq!(
        order,
        vec![
            "army.c-failed",
            "army.a-degraded",
            "army.b-degraded",
            "army.a-unknown",
            "army.b-healthy"
        ]
    );
}

/// The split is by prefix, and it has to hold for the ids on main today as well as the
/// ones Process 3 is renaming to.
#[test]
fn a_board_shows_only_its_own_group() {
    let all = vec![
        d("army.carl", Health::Healthy),
        d("system.cpu", Health::Healthy),
        d("agent.nora", Health::Healthy),
        d("system.disk:/", Health::Healthy),
    ];
    assert_eq!(
        sorted(&all, "army").len(),
        2,
        "army and the legacy agent id"
    );
    assert_eq!(sorted(&all, "system").len(), 2);
    assert_eq!(BOARDS.len(), 2, "two boards, and the army is read first");
    assert_eq!(BOARDS[0].0, "army");
}

/// An unreadable metric renders as the word unknown, never as a zero somebody could act on.
#[test]
fn an_unreadable_metric_never_renders_as_a_number() {
    use carl::providers::health::{Metric, Reading};
    let gpu = Diagnostic::new("system.gpu", Health::Unknown, "no card", Kind::Sampled)
        .with(Metric::new("vram", Reading::Unknown, "MiB"));

    let pairs = gpu.metric_pairs();
    assert_eq!(pairs.len(), 1);
    assert!(
        pairs[0].1.to_lowercase().contains("unknown"),
        "got {:?}, which a reader could mistake for a measurement",
        pairs[0].1
    );
    assert!(!pairs[0].1.contains('0'), "and it must not read as zero");
}

/// A board with nothing on it and a board where everything is fine are different facts, and
/// the summary over the board has to be able to tell them apart.
#[test]
fn an_empty_board_has_no_worst_rather_than_a_healthy_one() {
    let all = vec![d("army.tasks", Health::Healthy)];
    assert_eq!(worst_on(&all, "army"), Some(Health::Healthy));
    assert_eq!(
        worst_on(&all, "system"),
        None,
        "nothing has reported at all"
    );
}

/// The tally has to count every health separately, in the order the board is read, and never
/// fold unknown into anything.
#[test]
fn the_tally_keeps_every_health_apart() {
    let all = vec![
        d("system.a", Health::Unknown),
        d("system.b", Health::Unknown),
        d("system.c", Health::Healthy),
        d("system.d", Health::Degraded),
        d("army.e", Health::Failed),
    ];
    let counts = tally(&all, "system");
    assert_eq!(
        counts[0],
        (Health::Failed, 0),
        "the army failure is not on this board"
    );
    assert_eq!(counts[2], (Health::Degraded, 1));
    assert_eq!(counts[3], (Health::Unknown, 2));
    assert_eq!(counts[4], (Health::Healthy, 1));

    let order: Vec<u8> = counts.iter().map(|(h, _)| worst_first(*h)).collect();
    assert_eq!(
        order,
        vec![0, 1, 2, 3, 4],
        "the tally reads worst first too"
    );
}

/// The board is already headed with the prefix, so repeating it on every row is seven
/// characters of nothing. A component that is not prefixed keeps its whole name.
#[test]
fn the_row_drops_the_prefix_the_board_already_carries() {
    assert_eq!(short_name("system.disk:/"), "disk:/");
    assert_eq!(short_name("army.agent-processes"), "agent-processes");
    assert_eq!(
        short_name("agent.nora"),
        "agent.nora",
        "not one of the two prefixes"
    );
    assert_eq!(short_name("something-else"), "something-else");
}
