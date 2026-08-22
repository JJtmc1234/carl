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

    /// Every task, rebuilt from what happened to it.
    pub fn tasks(&self) -> Result<BTreeMap<TaskId, Task>> {
        Ok(rebuild(&read(self.journal.path())?))
    }

    pub fn get(&self, id: &TaskId) -> Result<Option<Task>> {
        Ok(self.tasks()?.remove(id))
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
        let by = by.to_string();

        if by != task.created_by {
            return Err(Error::Refused(format!(
                "{by} cannot hand down a task {} created",
                task.created_by
            )));
        }

        self.journal.decide_and_append(move |records| {
            let tasks = rebuild(records);
            if tasks.contains_key(&id) {
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
        self.write(id, {
            let by = by.to_string();
            let id = id.clone();
            move |task| {
                let mut task = task.clone();
                let from = task.status;
                task.advance(&by, next)?;
                Ok(Some((by.clone(), Event::moved(&id, from, next))))
            }
        })
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

    /// Everything one agent is carrying that is not finished, oldest first.
    pub fn holding(&self, agent: &str) -> Result<Vec<Task>> {
        Ok(holding(&self.tasks()?, agent))
    }

    /// The record itself, for a reader that wants the history rather than the state.
    pub fn records(&self) -> Result<Vec<Record>> {
        read(self.journal.path())
    }

    /// The shared shape of every guarded move: find the task, decide, write.
    fn write(
        &mut self,
        id: &TaskId,
        decide: impl FnOnce(&Task) -> Result<Option<(String, Event)>>,
    ) -> Result<Task> {
        let wanted = id.clone();
        self.journal.decide_and_append(move |records| {
            let tasks = rebuild(records);
            let task = tasks.get(&wanted).ok_or_else(|| {
                Error::Refused(format!(
                    "there is no task {wanted} in the record, so nothing can be done to it"
                ))
            })?;
            decide(task)
        })?;

        self.get(id)?
            .ok_or_else(|| Error::Refused(format!("{id} vanished from the record")))
    }
}

/// Rebuilds every task from the events that created and moved it.
///
/// A task whose `Delegated` line is missing is skipped rather than invented. There is nowhere
/// else the goal and the conditions could come from, and a task with an empty goal would be one
/// nobody could review, which is the failure the whole design is shaped to avoid.
pub fn rebuild(records: &[Record]) -> BTreeMap<TaskId, Task> {
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
            } => {
                // Already there means a repeated line, and the first one is the one that
                // happened. Overwriting would reset the status of a task that has since moved.
                if tasks.contains_key(task) {
                    continue;
                }
                let Ok(verification) = Verification::of(must.clone()) else {
                    continue;
                };
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
                        workspace: None,
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
    tasks
}

/// Everything one agent is carrying that is not finished, in the order it was handed down.
fn holding(tasks: &BTreeMap<TaskId, Task>, agent: &str) -> Vec<Task> {
    tasks
        .values()
        .filter(|t| t.owner == agent && !t.status.settled())
        .cloned()
        .collect()
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
