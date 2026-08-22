//! Rebuilding tasks from the record, because there is nowhere else they exist.
//!
//! A `Task` is never written to disk. It lives in memory for the length of a chain run and then
//! the process ends. The journal outlives it, and `Delegated` carries the goal, the parent and
//! the verification conditions precisely so that this fold can put the task back together.
//!
//! This is what "the panel is not a second source of truth" means in practice. There is no task
//! table to keep in step with the army, because the fold is rerun rather than updated. A task
//! view that is wrong is a bug in twenty lines of folding, not two stores disagreeing.
//!
//! Rerunning the whole fold is correct and is fine at this size. It is wrong at a million lines,
//! and that is written here so whoever hits it knows it was a choice rather than an oversight.

use std::collections::BTreeMap;

use super::view::{Maybe, Review, TaskView};
use crate::army::event::{Event, Intervention, Record};

/// Every task the record knows about, oldest delegation first.
pub fn fold(records: &[Record]) -> Vec<TaskView> {
    let mut by_id: BTreeMap<String, TaskView> = BTreeMap::new();
    // Insertion order, kept separately so the result is in the order work was handed out rather
    // than in id order, which is random and would reshuffle the screen on every id.
    let mut order: Vec<String> = Vec::new();

    for r in records {
        match &r.event {
            Event::Delegated {
                task,
                to,
                goal,
                parent,
                must,
                project,
                ..
            } => {
                let id = task.to_string();
                if !by_id.contains_key(&id) {
                    order.push(id.clone());
                }
                by_id.insert(
                    id.clone(),
                    TaskView {
                        id,
                        goal: goal.clone(),
                        owner: to.clone(),
                        assigner: r.actor.clone(),
                        parent: parent.as_ref().map(|p| p.to_string()),
                        project: project.clone(),
                        // A task exists as assigned the moment it is handed over. Anything else
                        // comes from a later `Moved`.
                        status: "assigned".to_string(),
                        attempts: 0,
                        must: must.clone(),
                        review: Maybe::Unknown,
                        delegated_at: r.at,
                        updated_at: r.at,
                    },
                );
            }
            Event::Moved { task, to, .. } => {
                if let Some(v) = by_id.get_mut(task.as_str()) {
                    v.status.clone_from(to);
                    v.updated_at = r.at;
                }
            }
            Event::Submitted { task, attempt, .. } => {
                if let Some(v) = by_id.get_mut(task.as_str()) {
                    v.attempts = *attempt;
                    v.updated_at = r.at;
                }
            }
            Event::Reviewed {
                task,
                accepted,
                why,
            } => {
                if let Some(v) = by_id.get_mut(task.as_str()) {
                    v.review = Maybe::known(Review {
                        accepted: *accepted,
                        why: why.clone(),
                        by: r.actor.clone(),
                        at: r.at,
                    });
                    v.updated_at = r.at;
                }
            }
            // JJ reaching past the chain still moves the task, and the fold has to see it or the
            // panel would show a stopped task as running. It is folded in as a status the same
            // way a `Moved` would be, while the record of *who* decided stays an `Intervened`.
            Event::Intervened { what } => match what {
                Intervention::Stopped { task, .. } => {
                    if let Some(v) = by_id.get_mut(task.as_str()) {
                        v.status = "abandoned".to_string();
                        v.updated_at = r.at;
                    }
                }
                Intervention::Replaced { task, goal, .. } => {
                    if let Some(v) = by_id.get_mut(task.as_str()) {
                        v.goal.clone_from(goal);
                        v.status = "assigned".to_string();
                        v.updated_at = r.at;
                    }
                }
                _ => {}
            },
            // Not a task between them. The runtime half of the vocabulary is about processes,
            // and an agent crashing is deliberately not a thing that happens to a task.
            Event::Refused { .. }
            | Event::EmergencyDeclared { .. }
            | Event::Decided { .. }
            | Event::Notified { .. }
            | Event::AgentStarted { .. }
            | Event::AgentCrashed { .. }
            | Event::AgentStartFailed { .. }
            | Event::AgentStopped { .. }
            | Event::AgentGaveUp { .. }
            | Event::AgentWoken { .. }
            | Event::ContinuityChanged { .. }
            // A grant is about a task and is still not a change to it. Showing it as a status
            // would put a permission in the column that says how far the work has got.
            | Event::Granted { .. } => {}
        }
    }

    order
        .into_iter()
        .filter_map(|id| by_id.remove(&id))
        .collect()
}

/// The task this agent is currently accountable for, if the record says there is one.
pub fn held_by<'a>(tasks: &'a [TaskView], agent: &str) -> Option<&'a TaskView> {
    tasks
        .iter()
        .rev()
        .find(|t| t.owner == agent && !settled(&t.status))
}

/// Whether a status means the task is finished with, by name rather than by type.
///
/// The fold works in strings because that is what the record holds, and `Status` is only
/// reachable from a live task. The two lists are kept in step by a test rather than by hoping.
pub fn settled(status: &str) -> bool {
    matches!(status, "accepted" | "abandoned")
}
