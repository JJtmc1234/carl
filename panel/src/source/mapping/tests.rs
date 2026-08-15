//! The mapping is where honesty is either kept or quietly lost, so it is tested on its own.

use super::*;
use crate::model::{Health, Phase, Reading};
use carl::panel::view::{
    AgentView as WireAgent, CarlView, DiagnosticView, Health as WireHealth, LastEvent, Maybe,
    Metric, Milestone as WireMilestone, PanelSnapshot, ProjectView, TaskView,
};

fn agent(name: &str, enlisted: bool) -> WireAgent {
    WireAgent {
        name: name.into(),
        display: name.into(),
        rank: carl::army::org::find(name)
            .map(|a| a.rank)
            .unwrap_or(carl::army::Rank::Worker),
        remit: String::new(),
        reports_to: None,
        department: None,
        sub_department: None,
        enlisted,
        holding: None,
        task_status: Maybe::Unknown,
        blocked: Maybe::Unknown,
        last_event: Maybe::Unknown,
        model: Maybe::Unknown,
        process: Maybe::Unknown,
    }
}

fn task(id: &str, owner: &str, status: &str) -> TaskView {
    TaskView {
        id: id.into(),
        goal: "fix the belt rate".into(),
        owner: owner.into(),
        assigner: "mason".into(),
        parent: None,
        status: status.into(),
        attempts: 1,
        must: vec!["tests pass".into()],
        review: Maybe::Unknown,
        delegated_at: 100,
        updated_at: 200,
    }
}

/// The distinction the whole `Maybe` type exists for must survive the mapping.
#[test]
fn nobody_looked_is_not_the_same_as_nothing_there() {
    let unlooked = one_agent(&agent("nora", true), &[]);
    assert_eq!(
        unlooked.model, None,
        "unknown becomes absent, never a default"
    );
    assert_eq!(unlooked.process, None);
    assert_eq!(unlooked.last_activity, None);
    assert_eq!(unlooked.blocker, None);

    let mut known = agent("nora", true);
    known.model = Maybe::known("claude-opus-5".into());
    known.last_event = Maybe::known(LastEvent {
        seq: 4,
        at: 900,
        kind: "submitted".into(),
        task: None,
    });
    let told = one_agent(&known, &[]);
    assert_eq!(told.model.as_deref(), Some("claude-opus-5"));
    assert_eq!(told.last_activity.as_deref(), Some("submitted"));
    assert_eq!(told.last_activity_at, Some(900));
}

/// An agent nobody has enlisted is unknown, which is a different fact from idle.
#[test]
fn an_unenlisted_agent_is_unknown_rather_than_idle() {
    assert_eq!(
        one_agent(&agent("nora", false), &[]).status,
        crate::model::AgentStatus::Unknown
    );
    assert_eq!(
        one_agent(&agent("nora", true), &[]).status,
        crate::model::AgentStatus::Idle
    );
}

#[test]
fn the_held_task_decides_what_the_agent_is_doing() {
    let tasks = vec![task("t1", "nora", "submitted")];
    let mut wire = agent("nora", true);
    wire.holding = Some("t1".into());
    assert_eq!(
        one_agent(&wire, &tasks).status,
        crate::model::AgentStatus::AwaitingReview
    );

    let tasks = vec![task("t1", "nora", "in hand")];
    assert_eq!(
        one_agent(&wire, &tasks).status,
        crate::model::AgentStatus::Working
    );
}

/// Blocked beats whatever the task says, because it is the thing somebody has to act on.
#[test]
fn blocked_wins_and_names_the_task_it_is_stuck_on() {
    let tasks = vec![task("t1", "nora", "in hand")];
    let mut wire = agent("nora", true);
    wire.holding = Some("t1".into());
    wire.blocked = Maybe::known(true);

    let view = one_agent(&wire, &tasks);
    assert_eq!(view.status, crate::model::AgentStatus::Blocked);
    assert!(view.blocker.unwrap().contains("belt rate"));
}

/// The two boards are split here rather than upstream, from prefixes Process 3 agreed are stable.
#[test]
fn the_board_a_component_lands_on_comes_from_its_prefix() {
    assert_eq!(diagnostics::group_of("system.cpu"), "system");
    assert_eq!(diagnostics::group_of("system.gpu"), "system");
    assert_eq!(diagnostics::group_of("army.blockers"), "army");
    assert_eq!(diagnostics::group_of("carl.process"), "army");
    assert_eq!(diagnostics::group_of("agent.nora"), "army");
    assert_eq!(
        diagnostics::group_of("weird"),
        "army",
        "anything unknown is army"
    );
}

/// A machine number decays and army state does not, so only one of them carries an age.
#[test]
fn only_machine_readings_are_sampled() {
    assert_eq!(diagnostics::reading_of("system.cpu"), Reading::Sampled);
    assert_eq!(diagnostics::reading_of("army.tasks"), Reading::EventDriven);
}

/// Process 3 keeps a sampled unknown carrying its timestamp and metric names on purpose, and
/// the panel must draw that as a gap without ever turning it into a zero.
#[test]
fn an_unknown_reading_keeps_its_detail_and_never_becomes_zero() {
    let wire = DiagnosticView {
        component: "system.gpu".into(),
        health: WireHealth::Unknown,
        summary: "no NVIDIA card present".into(),
        measured_at: 1_760_000_000,
        metrics: vec![Metric {
            name: "vram".into(),
            value: 0.0,
            unit: Some("MB".into()),
        }],
    };
    let drawn = one_diagnostic(&wire);

    assert_eq!(drawn.health, Health::Unknown);
    assert_eq!(
        drawn.measured_at,
        Some(1_760_000_000),
        "looked at and found nothing is not the same as never looked"
    );
    assert_eq!(
        drawn.summary, "no NVIDIA card present",
        "the reason survives"
    );
    assert_eq!(drawn.metrics.len(), 1, "the metric name survives");
    assert!(!drawn.health.wants_attention(), "a gap is not an alarm");
}

/// Zero is not a time anybody measured at, so it reads back as the absence it stands for.
#[test]
fn a_zero_timestamp_means_never_measured() {
    let wire = DiagnosticView {
        component: "system.temperature".into(),
        health: WireHealth::Unknown,
        summary: "no sensor".into(),
        measured_at: 0,
        metrics: Vec::new(),
    };
    let drawn = one_diagnostic(&wire);
    assert_eq!(drawn.measured_at, None);
    assert_eq!(drawn.age_secs(500), None, "and therefore has no age");
}

/// A phase spelled differently is unknown rather than the nearest guess.
#[test]
fn an_unrecognised_phase_is_unknown_rather_than_a_guess() {
    assert_eq!(projects::phase_of("building"), Phase::Building);
    assert_eq!(projects::phase_of("  Verifying "), Phase::Verifying);
    assert_eq!(projects::phase_of("gardening"), Phase::Unknown);
    assert_eq!(projects::phase_of(""), Phase::Unknown);
}

/// Nothing about a project is inferred. What the backend did not say stays empty.
#[test]
fn a_project_carries_only_what_the_backend_said() {
    let wire = ProjectView {
        id: "p1".into(),
        name: "jjtorio".into(),
        goal: "a factorio mod".into(),
        phase: "building".into(),
        department: Some("coding".into()),
        active_tasks: vec!["t1".into()],
        blockers: vec!["no lua".into()],
        milestones: vec![WireMilestone {
            at: 500,
            what: "belts verified".into(),
        }],
    };
    let drawn = one_project(&wire);

    assert_eq!(drawn.phase, Phase::Building);
    assert_eq!(drawn.department.as_deref(), Some("coding"));
    assert_eq!(
        drawn.owner, None,
        "a department is not an accountable agent"
    );
    assert_eq!(drawn.status, None, "no status was sent, so none is drawn");
    assert_eq!(drawn.next_objective, None);
    assert!(
        drawn.active_agents.is_empty(),
        "the backend never said who is on it, so nobody is shown"
    );
    assert_eq!(drawn.active_tasks, vec!["t1".to_string()]);
    assert_eq!(drawn.milestones.len(), 1);
}

/// A whole snapshot, end to end, with the joins the screen relies on.
#[test]
fn a_backend_snapshot_becomes_a_drawable_one() {
    let wire = PanelSnapshot {
        seq: 12,
        at: 1_000,
        carl: CarlView {
            status: Maybe::Unknown,
            pending: vec![carl::panel::view::Pending {
                seq: 7,
                at: 900,
                asked_by: "carl".into(),
                question: "install pytest?".into(),
                task: None,
            }],
            objectives: vec!["fix the planner".into()],
            recent_delegations: vec![task("t1", "nora", "in hand")],
        },
        agents: vec![agent("nora", true)],
        tasks: vec![task("t1", "nora", "in hand")],
        projects: Vec::new(),
        diagnostics: Vec::new(),
    };

    let drawn = snapshot(wire);
    assert_eq!(drawn.at, 1_000);
    assert_eq!(drawn.agents.len(), 1);
    assert_eq!(drawn.tasks.len(), 1);
    assert_eq!(drawn.decisions.len(), 1);
    assert_eq!(drawn.decisions[0].id, "7", "the sequence is the answer key");
    assert!(
        drawn.decisions[0].options.is_empty(),
        "no options were offered, so none are invented"
    );
    assert_eq!(drawn.delegations.len(), 1);
    assert_eq!(drawn.delegations[0].from, "mason");
    assert_eq!(drawn.delegations[0].to, "nora");
    assert!(
        drawn.conversation.is_empty(),
        "the backend keeps no history, so none is fabricated"
    );
}
