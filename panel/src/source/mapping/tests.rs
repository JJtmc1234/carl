//! The mapping is where honesty is either kept or quietly lost, so it is tested on its own.
//!
//! Diagnostics and projects are no longer mapped at all: they are Process 3's canonical types
//! on the wire and on the screen. What is left here is the agent overlay, which is genuinely
//! derived, and the snapshot joins the screen relies on.

use carl::panel::view::{
    AgentView as WireAgent, CarlView, LastEvent, Maybe, PanelSnapshot, TaskView,
};

use super::*;
use crate::model::AgentStatus;

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
        project: carl::ProjectId::new("jjtorio").ok(),
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
///
/// This is also what stops a process indicator going green because some Claude process exists
/// somewhere. Unknown stays unknown until something measured this one.
#[test]
fn an_unenlisted_agent_is_unknown_rather_than_idle() {
    assert_eq!(
        one_agent(&agent("nora", false), &[]).status,
        AgentStatus::Unknown
    );
    assert_eq!(
        one_agent(&agent("nora", true), &[]).status,
        AgentStatus::Idle
    );
}

#[test]
fn the_held_task_decides_what_the_agent_is_doing() {
    let mut wire = agent("nora", true);
    wire.holding = Some("t1".into());

    let submitted = vec![task("t1", "nora", "submitted")];
    assert_eq!(
        one_agent(&wire, &submitted).status,
        AgentStatus::AwaitingReview
    );

    let in_hand = vec![task("t1", "nora", "in hand")];
    assert_eq!(one_agent(&wire, &in_hand).status, AgentStatus::Working);
}

/// Blocked beats whatever the task says, because it is the thing somebody has to act on.
#[test]
fn blocked_wins_and_names_the_task_it_is_stuck_on() {
    let tasks = vec![task("t1", "nora", "in hand")];
    let mut wire = agent("nora", true);
    wire.holding = Some("t1".into());
    wire.blocked = Maybe::known(true);

    let view = one_agent(&wire, &tasks);
    assert_eq!(view.status, AgentStatus::Blocked);
    assert!(view.blocker.unwrap().contains("belt rate"));
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

/// The link a project pane walks is the one the record carries, not a guess.
#[test]
fn a_task_belongs_to_the_project_the_record_names() {
    let mut wire = PanelSnapshot {
        seq: 1,
        at: 1,
        carl: CarlView {
            status: Maybe::Unknown,
            pending: Vec::new(),
            objectives: Vec::new(),
            recent_delegations: Vec::new(),
        },
        agents: Vec::new(),
        tasks: vec![task("t1", "nora", "in hand"), task("t2", "nora", "in hand")],
        projects: Vec::new(),
        diagnostics: Vec::new(),
    };
    wire.tasks[1].project = None;

    let drawn = snapshot(wire);
    let jjtorio = carl::ProjectId::new("jjtorio").unwrap();
    let mine = drawn.tasks_in(&jjtorio);

    assert_eq!(mine.len(), 1, "only the task the record put in the project");
    assert_eq!(mine[0].id, "t1");
}
