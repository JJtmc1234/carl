//! Projects, narrowed for drawing.
//!
//! The rule that matters here is that nothing is inferred. The backend sends task ids and a
//! department. It does not say which agents are working on a project, so the panel shows none
//! rather than deriving it from who owns a task, and it does not say what the next objective is,
//! so that stays empty. A relationship nobody asserted is not a relationship.

use carl::panel::view::ProjectView;

use crate::model::{Milestone, Phase, Project};

/// A phase name, or `Unknown` when it is not one the screen knows.
///
/// Unrecognised is unknown rather than a guess at the nearest match. A project drawn as
/// building because its phase was spelled differently is worse than one drawn as unknown.
pub fn phase_of(phase: &str) -> Phase {
    match phase.trim().to_ascii_lowercase().as_str() {
        "planned" | "planning" => Phase::Planned,
        "building" | "active" | "in progress" => Phase::Building,
        "verifying" | "review" | "reviewing" => Phase::Verifying,
        "paused" | "on hold" | "held" => Phase::Paused,
        "done" | "finished" | "complete" | "completed" => Phase::Done,
        _ => Phase::Unknown,
    }
}

pub fn one_project(wire: &ProjectView) -> Project {
    Project {
        name: wire.name.clone(),
        goal: wire.goal.clone(),
        phase: phase_of(&wire.phase),
        // The backend sends no status line, so none is drawn.
        status: None,
        // A department is not an accountable agent, and folding one into the other would put a
        // name in the owner field that nobody assigned.
        owner: None,
        department: wire.department.clone(),
        // Only when the backend explicitly supplies them, which today it does not.
        active_agents: Vec::new(),
        active_tasks: wire.active_tasks.clone(),
        blockers: wire.blockers.clone(),
        milestones: wire
            .milestones
            .iter()
            .map(|m| Milestone {
                at: m.at,
                title: m.what.clone(),
                detail: None,
            })
            .collect(),
        next_objective: None,
    }
}
