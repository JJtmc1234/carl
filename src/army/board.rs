//! Every task the record knows about, and the only safe way to move one.
//!
//! A `Task` is a value. It has rules, and the rules are good, and none of them survive being
//! held by two processes at once. Carl restarts, rebuilds a task from the journal, finds it
//! waiting on review, and accepts it. Adrian's process does the same thing a moment later. Both
//! were right about what they read, both followed every rule in `task.rs`, and the task is now
//! finished twice.
//!
//! So a move is decided against the record rather than against a value in memory, with the
//! record held open across the decision. Whoever gets there second reads the first one's line
//! and is refused. That is what makes "a task completes exactly once" a property rather than a
//! hope.
//!
//! **No second copy on disk.** A task is rebuilt from the events that created and moved it, and
//! `Delegated` carries the goal, the conditions and the parent for exactly this reason. Writing
//! tasks to their own files as well would be two records of one thing, and two records of one
//! thing disagree the first time a write fails halfway.
//!
//! **The board owns no processes and starts nothing.** It is the other half of the line the
//! supervisor is on. The supervisor knows what is running and nothing about work. This knows
//! what the work is and nothing about what is running, and neither can answer the other's
//! question, which is why an agent dying can never mean a task succeeded.
//!
//! **Backups are not a scheduler.** A worker holds one task it is working on and up to three its
//! lead has already approved. When the first is blocked it may turn to the next approved one, in
//! the order they were handed down, and that is the entire rule. There is no priority, no
//! preemption and nothing that weighs one task against another, because all of those are
//! decisions and decisions belong to a lead.
//!
//! A worker cannot invent a backup, and there is no check that stops it, which is better. The
//! approved set is exactly the tasks already handed to it, and handing a task down goes through
//! `Task::assign`, which refuses anybody who is not the owner's boss.
//!
//! Not here, on purpose: priorities, scheduling, and anything that decides which task should be
//! done next. A lead decides that. This only refuses the moves that are not allowed.

use std::collections::BTreeMap;
use std::path::Path;

use crate::army::event::{Event, Journal, Record, read};
use crate::army::task::{Status, Task, TaskId, Verification};
use crate::{Error, Result};

/// How many tasks one agent may be holding at once, counting the one it is working on.
///
/// One primary and up to three approved backups. A worker with a longer list is a worker
/// choosing what to do, which is its lead's job, and a lead who can hand out fifteen has stopped
/// deciding anything.
pub const AT_ONCE: usize = 4;

/// What one agent is holding, split into the one it is doing and the ones it may turn to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The task this agent has started. `None` when it has started nothing, which is a worker
    /// that has been given work and has not picked it up rather than a worker with none.
    pub primary: Option<Task>,
    /// Everything else it is carrying, oldest first.
    pub backups: Vec<Task>,
}

/// The work, as the record has it.
pub struct Board {
    journal: Journal,
}

impl Board {
    /// Opens the record a home keeps its work in.
    pub fn open(home: &Path) -> Result<Self> {
        Self::at(home.join("run").join("events.jsonl"))
    }

    /// Opens a record at an exact path, for a run that keeps its own.
    pub fn at(path: impl Into<std::path::PathBuf>) -> Result<Self> {
        Ok(Self {
            journal: Journal::open(path)?,
        })
    }

    pub fn path(&self) -> &Path {
        self.journal.path()
    }

    /// Every task, rebuilt from what happened to it, in the order it was handed down.
    pub fn tasks(&self) -> Result<Vec<Task>> {
        Ok(rebuild(&read(self.journal.path())?))
    }

    pub fn get(&self, id: &TaskId) -> Result<Option<Task>> {
        Ok(self.tasks()?.into_iter().find(|t| t.id == *id))
    }

    /// Hands one task down, refusing it if the owner is already holding as many as it may.
    ///
    /// The chain of command and the shape of the task were checked when it was built. What is
    /// checked here is the one thing that cannot be known from a task on its own, which is how
    /// much its owner is already carrying.
    pub fn delegate(&mut self, by: &str, task: &Task) -> Result<()> {
        let owner = task.owner.clone();
        let id = task.id.clone();
        let goal = task.goal.clone();
        let must = task.verification.must.clone();
        let parent = task.parent.clone();
        let project = task.project.clone();
        let workspace = task.workspace.clone();
        let objective = task.objective;
        let by = by.to_string();

        if by != task.created_by {
            return Err(Error::Refused(format!(
                "{by} cannot hand down a task {} created",
                task.created_by
            )));
        }

        self.journal.decide_and_append(move |records| {
            let tasks = rebuild(records);
            if tasks.iter().any(|t| t.id == id) {
                return Err(Error::Refused(format!("{id} has already been handed down")));
            }

            let carrying = holding(&tasks, &owner).len();
            if carrying >= AT_ONCE {
                return Err(Error::Refused(format!(
                    "{owner} is already holding {carrying} tasks, which is as many as anybody \
                     may have at once. One is being worked on and the rest are approved backups. \
                     Finish or abandon one before handing down another."
                )));
            }

            Ok(Some((
                by,
                Event::Delegated {
                    task: id,
                    to: owner,
                    goal,
                    parent,
                    must,
                    project,
                    workspace,
                    objective,
                },
            )))
        })?;
        Ok(())
    }

    /// Moves a task, checked against the record at the moment of writing.
    ///
    /// Returns the task as it now stands. A move that is not allowed is an error naming why,
    /// including the case where somebody else got there first, because "it is already accepted"
    /// is the answer the caller asked for rather than a failure of theirs.
    pub fn advance(&mut self, by: &str, id: &TaskId, next: Status) -> Result<Task> {
        let wanted = id.clone();
        let by = by.to_string();

        self.journal.decide_and_append(move |records| {
            let tasks = rebuild(records);
            let task = tasks.iter().find(|t| t.id == wanted).ok_or_else(|| {
                Error::Refused(format!(
                    "there is no task {wanted} in the record, so nothing can be done to it"
                ))
            })?;

            // One at a time, checked against everything the owner is holding rather than against
            // the task being moved. A worker with two tasks in hand has to be told which to put
            // down, and nobody is in a position to tell it.
            //
            // In hand and not merely started. A blocked task is one the worker still owns and is
            // not working on, and treating it as busy would mean blocking made a worker idle,
            // which is the opposite of what backups are for.
            if next == Status::InHand
                && let Some(busy) = holding(&tasks, &task.owner)
                    .into_iter()
                    .find(|t| t.status == Status::InHand)
                && busy.id != wanted
            {
                return Err(Error::Refused(format!(
                    "{} has already started {}, and nobody works on two tasks at once. Block or \
                     finish that one before picking up {wanted}.",
                    task.owner, busy.id
                )));
            }

            let mut moving = task.clone();
            let from = moving.status;
            moving.advance(&by, next)?;
            Ok(Some((by.clone(), Event::moved(&wanted, from, next))))
        })?;

        self.get(id)?
            .ok_or_else(|| Error::Refused(format!("{id} vanished from the record")))
    }

    /// The owner says it is done and offers it for review.
    ///
    /// Two lines rather than one, because the attempt count and the size of what was produced
    /// are what a reviewer looks at first and neither can be read off a status.
    pub fn submit(&mut self, by: &str, id: &TaskId, words: usize) -> Result<Task> {
        let task = self.advance(by, id, Status::Submitted)?;
        self.journal.append(
            by,
            Event::Submitted {
                task: id.clone(),
                attempt: task.attempts,
                words,
            },
        )?;
        Ok(task)
    }

    /// Whoever assigned it decides, having checked.
    ///
    /// The verdict goes in whether it passed or not, because a review nobody can find is a
    /// review nobody did, and rejections are the half worth counting.
    pub fn review(&mut self, by: &str, id: &TaskId, accepted: bool, why: &str) -> Result<Task> {
        let next = match accepted {
            true => Status::Accepted,
            false => Status::ChangesRequested,
        };
        let task = self.advance(by, id, next)?;
        self.journal.append(
            by,
            Event::Reviewed {
                task: id.clone(),
                accepted,
                why: why.to_string(),
            },
        )?;
        Ok(task)
    }

    /// Records that a lead allowed its worker to do something, for one task.
    ///
    /// The grant, and not the enforcement. What stops a worker writing outside the directory it
    /// was given is the capability layer it runs against, in another process. Writing a path into
    /// a record stops nothing, and the two must not be confused: this answers "who allowed it",
    /// which is the question nobody can answer afterwards without a line like this.
    ///
    /// Only whoever assigned the task may grant against it. A worker widening its own permission
    /// and writing down that it did would otherwise look exactly like a lead deciding.
    pub fn grant(&mut self, by: &str, id: &TaskId, what: &str) -> Result<()> {
        let wanted = id.clone();
        let by = by.to_string();
        let what = what.to_string();

        self.journal.decide_and_append(move |records| {
            let tasks = rebuild(records);
            let task = tasks
                .iter()
                .find(|t| t.id == wanted)
                .ok_or_else(|| Error::Refused(format!("there is no task {wanted} to grant on")))?;

            if by != task.created_by {
                return Err(Error::Refused(format!(
                    "{by} cannot grant anything on {wanted}. It was assigned by {}, and only \
                     whoever assigned a task decides what doing it is allowed to touch.",
                    task.created_by
                )));
            }
            if task.status.settled() {
                return Err(Error::Refused(format!(
                    "{wanted} is {} and nothing more will be done to it, so there is nothing to \
                     allow.",
                    task.status
                )));
            }

            Ok(Some((
                by.clone(),
                Event::Granted {
                    task: wanted.clone(),
                    to: task.owner.clone(),
                    what: what.clone(),
                },
            )))
        })?;
        Ok(())
    }

    /// Everything one agent is carrying that is not finished, oldest first.
    pub fn holding(&self, agent: &str) -> Result<Vec<Task>> {
        Ok(holding(&self.tasks()?, agent))
    }

    /// What one agent is working on, and what it has been approved to turn to.
    pub fn standing(&self, agent: &str) -> Result<Standing> {
        let carrying = holding(&self.tasks()?, agent);
        let primary = started(&carrying).cloned();

        let backups = carrying
            .into_iter()
            .filter(|t| Some(&t.id) != primary.as_ref().map(|p| &p.id))
            .collect();

        Ok(Standing { primary, backups })
    }

    /// The backup this agent may pick up right now, if any.
    ///
    /// Only when what it is working on is blocked. Not when it is waiting on review, because
    /// review comes back and a worker holding two started tasks has to be told which to put down.
    /// Not when it has simply been given several, because then nothing has gone wrong and the
    /// first one is the one to do.
    ///
    /// The earliest approved backup, so which one it is does not depend on how a map happened to
    /// sort or on the worker's opinion.
    pub fn backup_for(&self, agent: &str) -> Result<Option<Task>> {
        let standing = self.standing(agent)?;
        let blocked = standing
            .primary
            .as_ref()
            .is_some_and(|p| p.status == Status::Blocked);

        if !blocked {
            return Ok(None);
        }
        Ok(standing
            .backups
            .into_iter()
            .find(|t| t.status == Status::Assigned))
    }

    /// The record itself, for a reader that wants the history rather than the state.
    pub fn records(&self) -> Result<Vec<Record>> {
        read(self.journal.path())
    }
}

/// Rebuilds every task from the events that created and moved it.
///
/// A task whose `Delegated` line is missing is skipped rather than invented. There is nowhere
/// else the goal and the conditions could come from, and a task with an empty goal would be one
/// nobody could review, which is the failure the whole design is shaped to avoid.
pub fn rebuild(records: &[Record]) -> Vec<Task> {
    let mut order: Vec<TaskId> = Vec::new();
    let mut tasks: BTreeMap<TaskId, Task> = BTreeMap::new();

    for record in records {
        match &record.event {
            Event::Delegated {
                task,
                to,
                goal,
                parent,
                must,
                project,
                workspace,
                objective,
            } => {
                // Already there means a repeated line, and the first one is the one that
                // happened. Overwriting would reset the status of a task that has since moved.
                if tasks.contains_key(task) {
                    continue;
                }
                let Ok(verification) = Verification::of(must.clone()) else {
                    continue;
                };
                order.push(task.clone());
                tasks.insert(
                    task.clone(),
                    Task {
                        id: task.clone(),
                        goal: goal.clone(),
                        verification,
                        status: Status::Assigned,
                        owner: to.clone(),
                        created_by: record.actor.clone(),
                        parent: parent.clone(),
                        attempts: 0,
                        project: project.clone(),
                        workspace: workspace.clone(),
                        objective: *objective,
                    },
                );
            }
            Event::Moved { task, to, .. } => {
                if let (Some(task), Some(status)) = (tasks.get_mut(task), status_named(to)) {
                    // Counted off the move rather than off the `Submitted` line, so the number
                    // is a fact about the record instead of a number somebody wrote into it. The
                    // two used to be able to disagree, and the way they did was quiet: whoever
                    // wrote the `Submitted` line had to read the count first, and reading it back
                    // before the move had been written gave zero every time.
                    if status == Status::Submitted {
                        task.attempts = task.attempts.saturating_add(1);
                    }
                    task.status = status;
                }
            }
            // Older lines, from before the count came off the moves. Taken as a floor rather
            // than as the answer, so an old journal still reads and a new one is not affected.
            Event::Submitted { task, attempt, .. } => {
                if let Some(task) = tasks.get_mut(task) {
                    task.attempts = task.attempts.max(*attempt);
                }
            }
            _ => {}
        }
    }

    // Handed down order, not id order. Ids are random hex, so a map's order is arbitrary, and
    // which backup a worker turns to next has to be the one its lead approved first rather than
    // whichever id happened to sort lowest.
    order
        .into_iter()
        .filter_map(|id| tasks.remove(&id))
        .collect()
}

/// Everything one agent is carrying that is not finished, in the order it was handed down.
fn holding(tasks: &[Task], agent: &str) -> Vec<Task> {
    tasks
        .iter()
        .filter(|t| t.owner == agent && !t.status.settled())
        .cloned()
        .collect()
}

/// The task in a list that its owner is on.
///
/// Whatever is in hand, and only then the earliest of the ones it has started and is not
/// currently working on. Preferring in hand matters once backups are in play: a worker whose
/// first task is blocked and who has picked up the next one is on the second, and answering with
/// the parked one would make every reader think it was still stuck.
fn started(tasks: &[Task]) -> Option<&Task> {
    tasks
        .iter()
        .find(|t| t.status == Status::InHand)
        .or_else(|| {
            tasks.iter().find(|t| {
                matches!(
                    t.status,
                    Status::Blocked | Status::ChangesRequested | Status::Submitted
                )
            })
        })
}

/// The status a `Moved` line names.
///
/// Matched on the words the events already carry rather than on a number, because the journal is
/// meant to be readable by somebody without this crate.
fn status_named(name: &str) -> Option<Status> {
    Some(match name {
        "assigned" => Status::Assigned,
        "in hand" => Status::InHand,
        "submitted" => Status::Submitted,
        "changes requested" => Status::ChangesRequested,
        "blocked" => Status::Blocked,
        "accepted" => Status::Accepted,
        "abandoned" => Status::Abandoned,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
