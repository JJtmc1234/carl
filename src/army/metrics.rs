//! Whether the army is getting better, folded out of the record it already keeps.
//!
//! Nine numbers, described in `docs/flagship-workflow.md`. Every one of them is a fold over
//! `run/events.jsonl` and none of them is recorded anywhere else, which is the whole design.
//! A second file counting objectives would be one failed write away from disagreeing with the
//! history, and the history is the thing anybody would believe.
//!
//! Nothing here estimates. There is no measure of time saved, because it would have to be
//! guessed at, a guess would flatter, and a flattering number is worse than a gap.
//!
//! Two of these are meant to look bad at first. Interventions per objective and the review
//! rejection rate both rise when the army starts doing work somebody cares about, because the
//! alternative is JJ not watching and leads not reading. A measure that only ever improves is
//! not being measured.

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use super::board::rebuild;
use super::event::{Event, Intervention, Record};
use super::personnel::AgentId;
use super::task::{Status, TaskId};

/// One thing JJ asked for, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub task: TaskId,
    pub goal: String,
    pub status: Status,
    /// Times JJ reached into this objective's tree after opening it.
    ///
    /// The opening objective is not one of these. Asking for something is not intervening in
    /// it, and counting it would put a floor of one under a number whose whole purpose is to
    /// reach zero.
    pub interventions: usize,
}

impl Objective {
    pub fn accepted(&self) -> bool {
        self.status == Status::Accepted
    }

    /// Finished, and JJ never had to touch it again.
    pub fn unattended(&self) -> bool {
        self.accepted() && self.interventions == 0
    }
}

/// How reviewing is going. Zero rejections is not a good sign.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Reviews {
    pub accepted: usize,
    pub rejected: usize,
}

/// Work handed down badly, seen from the end that had to do it again.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Retries {
    pub submissions: usize,
    /// Submissions that were not the first attempt at their task.
    pub repeats: usize,
}

/// Whether a crash is an event or an outage.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Recovery {
    pub crashes: usize,
    /// Crashes followed by that same agent starting again.
    pub resumed: usize,
    /// Agents the supervisor stopped trying to start.
    pub gave_up: usize,
    /// Crashed, and nothing has been recorded about it since.
    pub outstanding: usize,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Metrics {
    pub objectives: Vec<Objective>,
    pub reviews: Reviews,
    pub retries: Retries,
    /// A lead was let past the rank rules, which is the exception that must stay countable.
    pub escalations: usize,
    pub recovery: Recovery,
    /// An agent that came back with less than it had.
    pub continuity_failures: usize,
    /// A rule that actually stopped something. A rule nothing ever hits protects nothing.
    pub refusals: usize,
    /// Interventions that name no task, so they belong to no objective.
    ///
    /// A message straight to an agent, a standing override, or an answer to a question. Kept
    /// as their own number rather than spread over whatever happened to be open, because
    /// guessing which objective a sentence belonged to would put invented figures in the one
    /// place that is meant to be checkable.
    pub loose_interventions: usize,
}

impl Metrics {
    pub fn accepted(&self) -> usize {
        self.objectives.iter().filter(|o| o.accepted()).count()
    }

    pub fn unattended(&self) -> usize {
        self.objectives.iter().filter(|o| o.unattended()).count()
    }

    /// Interventions per objective, or nothing when there are no objectives yet.
    ///
    /// An average over nothing is not zero. Reporting it as zero would read as the best
    /// possible score for an army that has never been asked to do anything.
    pub fn interventions_each(&self) -> Option<f64> {
        if self.objectives.is_empty() {
            return None;
        }
        let total: usize = self.objectives.iter().map(|o| o.interventions).sum();
        Some(total as f64 / self.objectives.len() as f64)
    }

    /// The most recent `n`, which is where a trend lives.
    pub fn latest(&self, n: usize) -> &[Objective] {
        let from = self.objectives.len().saturating_sub(n);
        &self.objectives[from..]
    }
}

/// Everything the record says about how the army is doing, in order.
pub fn of(records: &[Record]) -> Metrics {
    let tasks = rebuild(records);

    // Which objective each task belongs to, so an intervention deep in a tree is counted
    // against the thing JJ actually asked for rather than against the subtask it landed on.
    let parents: BTreeMap<TaskId, Option<TaskId>> = tasks
        .iter()
        .map(|t| (t.id.clone(), t.parent.clone()))
        .collect();
    let root_of = |id: &TaskId| -> Option<TaskId> {
        let mut at = id.clone();
        // Bounded by the number of tasks, so a parent link that somehow forms a cycle stops
        // rather than hanging the report that would have shown it.
        for _ in 0..=parents.len() {
            match parents.get(&at) {
                Some(Some(up)) => at = up.clone(),
                Some(None) => return Some(at),
                None => return None,
            }
        }
        None
    };

    let mut objectives: BTreeMap<TaskId, Objective> = BTreeMap::new();
    let mut order: Vec<TaskId> = Vec::new();
    for task in tasks.iter().filter(|t| t.parent.is_none()) {
        order.push(task.id.clone());
        objectives.insert(
            task.id.clone(),
            Objective {
                task: task.id.clone(),
                goal: task.goal.clone(),
                status: task.status,
                interventions: 0,
            },
        );
    }

    let mut m = Metrics::default();
    // A crash waiting to be answered by a start or by the supervisor giving up.
    let mut wounded: BTreeMap<AgentId, ()> = BTreeMap::new();

    for record in records {
        match &record.event {
            Event::Reviewed { accepted, .. } => {
                if *accepted {
                    m.reviews.accepted += 1;
                } else {
                    m.reviews.rejected += 1;
                }
            }
            Event::Submitted { attempt, .. } => {
                m.retries.submissions += 1;
                if *attempt > 1 {
                    m.retries.repeats += 1;
                }
            }
            Event::EmergencyDeclared { .. } => m.escalations += 1,
            Event::Refused { .. } => m.refusals += 1,
            Event::ContinuityChanged { .. } => m.continuity_failures += 1,
            Event::AgentCrashed { agent, .. } => {
                m.recovery.crashes += 1;
                wounded.insert(agent.clone(), ());
            }
            Event::AgentStarted { agent, .. } => {
                if wounded.remove(agent).is_some() {
                    m.recovery.resumed += 1;
                }
            }
            Event::AgentGaveUp { agent, .. } => {
                m.recovery.gave_up += 1;
                wounded.remove(agent);
            }
            Event::Intervened { what } => match what {
                // The way in, not a reach past anybody. Counting it would mean every objective
                // starts one intervention behind.
                Intervention::Objective { .. } => {}
                Intervention::Stopped { task, .. } | Intervention::Replaced { task, .. } => {
                    match root_of(task).and_then(|root| objectives.get_mut(&root)) {
                        Some(objective) => objective.interventions += 1,
                        // A task the record never saw delegated. Counted rather than dropped,
                        // because an intervention nobody can place still happened.
                        None => m.loose_interventions += 1,
                    }
                }
                _ => m.loose_interventions += 1,
            },
            _ => {}
        }
    }

    m.recovery.outstanding = wounded.len();
    m.objectives = order
        .into_iter()
        .filter_map(|id| objectives.remove(&id))
        .collect();
    m
}
