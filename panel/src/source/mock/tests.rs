//! The mock has to be worth building against, which means it behaves like a backend rather
//! than like a fixture.

use std::time::Duration;

use super::*;
use crate::command::{Intervention, InterventionKind};
use crate::model::{Health, Kind};

#[test]
fn the_opening_snapshot_is_the_real_organisation() {
    let mut m = MockPanelDataSource::new();
    let snap = m.snapshot();

    let names: Vec<&str> = snap.agents.iter().map(|a| a.name.as_str()).collect();
    for agent in org::everyone() {
        assert!(names.contains(&agent.name), "{} missing", agent.name);
    }
    assert_eq!(
        snap.agents.len(),
        org::everyone().len(),
        "and nobody invented"
    );
}

/// The seed task has to be a real one, created through the real rules, or the detail view is
/// being built against something the army would never produce.
#[test]
fn the_seed_task_went_through_the_real_chain() {
    let mut m = MockPanelDataSource::new();
    let snap = m.snapshot();
    let task = snap.tasks.first().expect("a task");

    assert_eq!(task.assigner, "mason");
    assert!(
        task.project.is_some(),
        "the fixture task belongs to a project"
    );
    assert_eq!(task.owner, "nora");
    assert!(
        carl::army::org::may_delegate(&task.assigner, &task.owner),
        "the fixture must not invent a delegation the chain forbids"
    );
    assert!(!task.must.is_empty());
}

/// The board must include something nothing has measured, or the honest gap can never be seen.
#[test]
fn the_diagnostics_include_something_unmeasured() {
    let mut m = MockPanelDataSource::new();
    let snap = m.snapshot();

    assert!(
        snap.diagnostics
            .iter()
            .any(|d| d.health == Health::Unknown && d.measured_at.is_none()),
        "nothing on the board is unmeasured, so the unknown state is never exercised"
    );
    assert!(
        snap.diagnostics.iter().any(|d| d.kind == Kind::Sampled),
        "and nothing is sampled"
    );
    assert!(
        snap.diagnostics.iter().any(|d| d.kind == Kind::EventDriven),
        "and nothing is event driven"
    );
}

/// Nothing arrives before its time, and everything arrives eventually.
#[test]
fn the_timeline_delivers_in_order_and_only_when_due() {
    let mut m = MockPanelDataSource::new();
    assert!(m.poll().is_empty(), "nothing is due at zero");

    m.advance(Duration::from_secs(5));
    let first = m.poll();
    assert!(!first.is_empty(), "the first beat should have landed");
    assert!(m.poll().is_empty(), "and is not handed out twice");

    m.advance(Duration::from_secs(60));
    let rest = m.poll();
    assert!(!rest.is_empty(), "the remainder should follow");
}

/// A command produces a reply from the backend rather than being drawn on the way out.
#[test]
fn a_message_comes_back_as_an_echo_and_an_answer() {
    let mut m = MockPanelDataSource::new();
    m.submit(Command::SayToCarl("fix the belts".into()))
        .unwrap();

    assert!(m.poll().is_empty(), "nothing is instant");

    m.advance(Duration::from_millis(200));
    let echo = m.poll();
    assert!(
        echo.iter()
            .any(|e| matches!(e, PanelEvent::JjSaid(t) if t == "fix the belts")),
        "the message should be echoed back by the backend"
    );

    m.advance(Duration::from_secs(3));
    let answer = m.poll();
    assert!(
        answer
            .iter()
            .any(|e| matches!(e, PanelEvent::CarlSaid { .. })),
        "carl should answer"
    );
    let finished = answer
        .iter()
        .filter_map(|e| match e {
            PanelEvent::CarlSaid { streaming, .. } => Some(*streaming),
            _ => None,
        })
        .next_back();
    assert_eq!(finished, Some(false), "and the answer should end");
}

/// Nothing may be sent while the link is down, and the refusal has to say so.
#[test]
fn nothing_is_accepted_while_the_link_is_down() {
    let mut m = MockPanelDataSource::new();
    // Wind past the scripted disconnection.
    m.advance(Duration::from_secs(31));
    let _ = m.poll();
    assert!(!m.link().is_live());

    let err = m
        .submit(Command::SayToCarl("are you there".into()))
        .unwrap_err();
    assert!(err.contains("not sent"), "{err}");
    assert!(m.sent.is_empty(), "and nothing was recorded as sent");
}

/// An intervention is recorded as JJ acting, since that is the whole reason it is separate.
#[test]
fn an_intervention_is_recorded_against_jj() {
    let mut m = MockPanelDataSource::new();
    m.submit(Command::Intervene(Intervention {
        agent: "nora".into(),
        kind: InterventionKind::StopTask,
        body: "wrong task".into(),
    }))
    .unwrap();

    m.advance(Duration::from_millis(300));
    let events = m.poll();
    let record = events
        .iter()
        .find_map(|e| match e {
            PanelEvent::Recorded(r) => Some(r),
            _ => None,
        })
        .expect("a record");

    assert_eq!(record.actor, "jj", "an intervention is jj acting directly");
}

#[test]
fn the_source_says_what_it_is() {
    let m = MockPanelDataSource::new();
    assert!(m.describe().contains("mock"), "{}", m.describe());
}
