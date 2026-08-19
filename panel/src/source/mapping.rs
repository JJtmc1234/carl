//! Turning what the backend sends into what the panel draws.
//!
//! Process 1 and Process 3 both keep richer semantics than the screen needs, deliberately, and
//! this is where that richness is narrowed for drawing rather than destroyed at the source.
//! Nothing here asks either of them to send less.
//!
//! Three rules, and they are the whole reason this file is separate from the drawing.
//!
//! **Unknown never becomes a number.** `Maybe::Unknown` becomes `None`, an unrecognised phase
//! becomes `Phase::Unknown`, and a health nobody measured stays unknown. There is no branch in
//! this file that turns an absence into a zero, and a test asserts it.
//!
//! **Nothing is inferred.** The backend does not say which agents are on a project, so the
//! panel shows none rather than guessing from a task owner. It does not say what a worker's
//! worktree is, so that field is empty. A relationship nobody asserted is not drawn.
//!
//! **The group is derived from a prefix, not invented upstream.** `system.*` is the machine
//! board and everything else is the army board. Process 3 was right that `group` is the view
//! model's word, so it stays here.

use carl::panel::view::{
    AgentView as WireAgent, Maybe, PanelSnapshot, ProcessState as WireProcess, TaskView,
};

use crate::model::{AgentStatus, AgentView, Decision, Delegation, ProcessState, Snapshot};

/// Everything the backend sent, in the shape the screen draws.
pub fn snapshot(wire: PanelSnapshot) -> Snapshot {
    let tasks = wire.tasks;
    let agents = wire
        .agents
        .iter()
        .map(|a| one_agent(a, &tasks))
        .collect::<Vec<_>>();

    Snapshot {
        agents,
        // Both are Process 3's canonical types on the wire and on the screen, so there is
        // nothing to map. The panel used to convert them and lost a distinction each time.
        projects: wire.projects,
        diagnostics: wire.diagnostics,
        decisions: wire
            .carl
            .pending
            .iter()
            .map(|p| Decision {
                id: p.seq.to_string(),
                asked_at: p.at,
                question: p.question.clone(),
                detail: p.task.clone().map(|t| format!("about task {t}")),
                // The backend offers no options, so none are drawn. Inventing "yes" and "no"
                // would put words in Carl's mouth that he did not ask for.
                options: Vec::new(),
            })
            .collect(),
        delegations: wire
            .carl
            .recent_delegations
            .iter()
            .map(|t| Delegation {
                at: t.delegated_at,
                from: t.assigner.clone(),
                to: t.owner.clone(),
                goal: t.goal.clone(),
                task: Some(t.id.clone()),
            })
            .collect(),
        // The backend keeps no conversation history, so a fresh connection starts with an empty
        // Carl tab and fills as JJ talks. Shown as empty rather than back filled from the
        // journal, because a delegation record is not something Carl said.
        conversation: Vec::new(),
        tasks,
        events: Vec::new(),
        at: wire.at,
        seq_at: wire.seq,
    }
}

/// One agent, with the live overlay the screen needs derived from what the backend knows.
pub fn one_agent(wire: &WireAgent, tasks: &[TaskView]) -> AgentView {
    let task = wire.holding.clone();
    let held = task
        .as_ref()
        .and_then(|id| tasks.iter().find(|t| &t.id == id));

    AgentView {
        name: wire.name.clone(),
        department: wire.department.clone(),
        sub_department: wire.sub_department.clone(),
        status: status_of(wire, held),
        task,
        blocker: blocker_of(wire, held),
        last_activity: match &wire.last_event {
            Maybe::Known { value } => Some(value.kind.clone()),
            Maybe::Unknown => None,
        },
        last_activity_at: match &wire.last_event {
            Maybe::Known { value } => Some(value.at),
            Maybe::Unknown => None,
        },
        model: known(&wire.model),
        process: match &wire.process {
            Maybe::Known { value } => Some(match value {
                WireProcess::Running => ProcessState::Running,
                _ => ProcessState::Stopped,
            }),
            Maybe::Unknown => None,
        },
        // The backend does not carry either. Left empty rather than guessed at from a path.
        worktree: None,
        branch: None,
    }
}

/// What state to show an agent in.
///
/// Unknown is reached by not being enlisted or by the backend saying nothing, and it is a real
/// answer rather than a fallback. An agent with a folder and no task is idle, which is a
/// different fact from one nobody has looked at.
fn status_of(wire: &WireAgent, held: Option<&TaskView>) -> AgentStatus {
    if !wire.enlisted {
        return AgentStatus::Unknown;
    }
    if matches!(wire.blocked, Maybe::Known { value: true }) {
        return AgentStatus::Blocked;
    }
    match held.map(|t| t.status.as_str()) {
        Some("submitted") => AgentStatus::AwaitingReview,
        Some("in hand") => AgentStatus::Working,
        Some("assigned") | Some("changes requested") => AgentStatus::Working,
        Some(_) => AgentStatus::Idle,
        None => match &wire.task_status {
            Maybe::Known { value } if value == "submitted" => AgentStatus::AwaitingReview,
            Maybe::Known { value } if value == "in hand" => AgentStatus::Working,
            Maybe::Known { .. } => AgentStatus::Idle,
            Maybe::Unknown => AgentStatus::Idle,
        },
    }
}

/// The sentence shown when an agent is stopped.
///
/// The backend says whether it is blocked, not why, so the screen says the fact it was given
/// and does not compose a reason nobody sent.
fn blocker_of(wire: &WireAgent, held: Option<&TaskView>) -> Option<String> {
    match wire.blocked {
        Maybe::Known { value: true } => Some(match held {
            Some(t) => format!("blocked on {}", t.goal),
            None => "blocked, and the backend did not say why".into(),
        }),
        _ => None,
    }
}

fn known<T: Clone>(m: &Maybe<T>) -> Option<T> {
    match m {
        Maybe::Known { value } => Some(value.clone()),
        Maybe::Unknown => None,
    }
}

#[cfg(test)]
mod tests;
