//! Tests build a snapshot from the default and set the one field under test, which reads
//! better than restating every field of a large struct in each case.
#![allow(clippy::field_reassign_with_default)]

//! The counting rules, checked without a window.

use super::*;
use crate::model::{AgentView, Decision, Diagnostic, Kind};
use crate::source::MockPanelDataSource;
use crate::source::PanelDataSource;

fn snapshot() -> Snapshot {
    MockPanelDataSource::new().snapshot()
}

fn diag(component: &str, health: Health) -> Diagnostic {
    Diagnostic::new(component, health, "why", Kind::EventDriven)
}

/// JJ is not in the army, so he must never turn up in a count of who is working or idle.
/// Counting him made the idle figure wrong by exactly one, forever, which is the kind of
/// error nobody notices and everybody half trusts.
#[test]
fn the_human_is_not_counted_as_an_agent() {
    let mut s = snapshot();
    let v = read(&s);
    assert_eq!(
        v.agents(),
        s.agents.len() - 1,
        "everybody except JJ is an agent"
    );

    // Even with JJ in a state that would otherwise be counted.
    for a in s.agents.iter_mut().filter(|a| a.name == "jj") {
        a.status = AgentStatus::Working;
    }
    assert_eq!(
        read(&s).working,
        v.working,
        "JJ working is not army capacity"
    );
    assert!(is_human("jj"));
    assert!(!is_human("nora"));
}

/// A component nothing has measured is its own number. Folding it into healthy would let the
/// headline claim something nobody checked, which is the exact lie this panel exists to avoid.
#[test]
fn unmeasured_is_its_own_count_and_never_healthy() {
    let mut s = Snapshot::default();
    s.diagnostics = vec![
        diag("system.gpu", Health::Unknown),
        diag("army.tasks", Health::Healthy),
    ];
    let v = read(&s);
    assert_eq!(v.unmeasured, 1);
    assert_eq!(v.healthy, 1);
    assert_eq!(v.worst(), Health::Healthy, "a gap is not a fault");

    // And with nothing measured at all the headline must say so rather than say all clear.
    let empty = Vitals::default();
    assert_eq!(empty.worst(), Health::Unknown);
    assert!(
        empty.headline().0.contains("MEASURED"),
        "{:?}",
        empty.headline().0
    );
}

/// The headline reports the worst thing there is, and it is never carried by colour alone.
#[test]
fn the_headline_reports_the_worst_thing_and_carries_a_word_and_a_shape() {
    let mut s = Snapshot::default();
    s.diagnostics = vec![diag("a", Health::Healthy)];
    assert_eq!(read(&s).worst(), Health::Healthy);

    s.diagnostics.push(diag("b", Health::Degraded));
    assert_eq!(read(&s).worst(), Health::Degraded);

    s.diagnostics.push(diag("c", Health::Blocked));
    assert_eq!(read(&s).worst(), Health::Blocked);

    s.diagnostics.push(diag("d", Health::Failed));
    let v = read(&s);
    assert_eq!(v.worst(), Health::Failed);

    let (word, colour, shape) = v.headline();
    assert!(
        !word.is_empty(),
        "a headline with no word is a coloured bar"
    );
    assert_eq!(colour, theme::BAD);
    assert_eq!(shape, Mark::Cross);
}

/// A blocked agent is a held up army even when every machine reading is fine, because the
/// question the headline answers is about the organisation and not about the hardware.
#[test]
fn a_blocked_agent_is_enough_to_stop_the_headline_saying_all_is_well() {
    let mut s = Snapshot::default();
    s.diagnostics = vec![diag("a", Health::Healthy)];
    s.agents = vec![AgentView::unknown("nora")];
    s.agents[0].status = AgentStatus::Blocked;
    assert_eq!(read(&s).worst(), Health::Blocked);
}

/// Worst first, and the order is the one written down in the doc comment rather than whatever
/// the iteration happened to produce.
#[test]
fn what_needs_jj_comes_out_worst_first() {
    let mut s = Snapshot::default();
    s.diagnostics = vec![
        diag("system.disk", Health::Degraded),
        diag("army.x", Health::Failed),
    ];
    s.agents = vec![AgentView::unknown("nora")];
    s.agents[0].status = AgentStatus::Blocked;
    s.agents[0].blocker = Some("a dependency is missing".into());
    s.decisions = vec![Decision {
        id: "d1".into(),
        asked_at: 0,
        question: "install pytest?".into(),
        detail: None,
        options: vec![],
    }];

    let kinds: Vec<&str> = needs(&s).iter().map(|n| n.kind).collect();
    assert_eq!(
        kinds,
        vec!["CARL ASKS", "FAILED", "BLOCKED", "DEGRADED"],
        "a question already waiting on a person outranks everything"
    );
}

/// Nothing wrong means an empty list rather than a list of reassurances. A panel that always
/// has five rows in its attention pane teaches somebody to stop reading it.
#[test]
fn a_quiet_army_needs_nothing() {
    let mut s = Snapshot::default();
    s.diagnostics = vec![diag("a", Health::Healthy), diag("b", Health::Unknown)];
    assert!(
        needs(&s).is_empty(),
        "an unmeasured component is not a call to action"
    );
    assert_eq!(read(&s).wants_jj(), 0);
}

/// The real fixture, so the numbers on the mock screen can be reasoned about.
#[test]
fn the_mock_army_reports_what_the_fixture_actually_contains() {
    let s = snapshot();
    let v = read(&s);
    assert_eq!(
        v.agents(),
        carl::army::org::everyone()
            .iter()
            .filter(|a| a.rank != carl::army::org::Rank::Human)
            .count(),
        "everybody in the table except JJ"
    );
    assert_eq!(v.working, 1, "carl is the only one moving at the start");
    assert!(v.unmeasured >= 2, "the fixture carries deliberate gaps");
    assert_eq!(v.projects_active, 2);
    assert_eq!(v.projects_blocked, 1);
}
