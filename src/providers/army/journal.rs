//! What the army did, folded into what is true now.
//!
//! The journal is the only cross process record the army has, so it is the only place a panel
//! running outside Carl can learn that a task is stuck. This module folds it and derives
//! nothing that the journal does not already say.
//!
//! **It does not keep task state.** `army::task::Task` owns status and is authoritative while a
//! chain is running in its own process. What is folded here is the trail that chain left
//! behind, which is a different thing and is labelled as such. When the two disagree the task
//! is right and this is stale, which is exactly why the panel shows the journal's own age.

use std::collections::BTreeMap;

use crate::army::event::{self, Event, Record};
use crate::army::task::{MAX_ATTEMPTS, TaskId};
use crate::providers::health::Health;

/// What the journal says about one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTrail {
    pub id: TaskId,
    pub goal: Option<String>,
    pub owner: Option<String>,
    /// The `to` of the last `Moved`, as the journal spelled it.
    pub status: Option<String>,
    pub attempts: u32,
    pub rejections: u32,
    pub accepted: bool,
    pub emergency: bool,
    pub first_at: u64,
    pub last_at: u64,
}

impl TaskTrail {
    /// Whether this task has run out of attempts and is waiting on somebody above.
    ///
    /// The same rule `Task::must_escalate` uses, read off the trail rather than reimplemented
    /// with a different number: `MAX_ATTEMPTS` comes from `army::task`.
    pub fn must_escalate(&self) -> bool {
        self.status.as_deref() == Some("changes requested") && self.attempts >= MAX_ATTEMPTS
    }

    pub fn settled(&self) -> bool {
        matches!(self.status.as_deref(), Some("accepted") | Some("abandoned"))
    }

    /// Five ways rather than two.
    ///
    /// A task sent back for changes is degraded and still moving. The same task with no
    /// attempts left is blocked, because nobody below can do anything about it. Abandoned is
    /// the only failure, because it is the only status a task does not come back from.
    pub fn health(&self) -> Health {
        match self.status.as_deref() {
            Some("accepted") => Health::Healthy,
            Some("abandoned") => Health::Failed,
            Some("changes requested") if self.must_escalate() => Health::Blocked,
            Some("changes requested") => Health::Degraded,
            Some(_) => Health::Healthy,
            None => Health::Unknown,
        }
    }

    /// How long the task has been on the books, in seconds.
    pub fn elapsed(&self) -> u64 {
        self.last_at.saturating_sub(self.first_at)
    }
}

/// Everything the journal knows, folded.
#[derive(Debug, Clone, Default)]
pub struct Folded {
    pub tasks: Vec<TaskTrail>,
    /// `Refused` events, which are the record of somebody being stopped.
    pub refusals: Vec<(u64, String, String)>,
    pub records: usize,
    /// Lines that were in the file and could not be parsed.
    pub unreadable_lines: usize,
    pub last_seq: Option<u64>,
    pub last_at: Option<u64>,
}

impl Folded {
    pub fn blocked(&self) -> Vec<&TaskTrail> {
        self.tasks
            .iter()
            .filter(|t| t.health() == Health::Blocked)
            .collect()
    }

    pub fn active(&self) -> Vec<&TaskTrail> {
        self.tasks.iter().filter(|t| !t.settled()).collect()
    }

    pub fn failed(&self) -> Vec<&TaskTrail> {
        self.tasks
            .iter()
            .filter(|t| t.health() == Health::Failed)
            .collect()
    }

    /// The seconds between a task being handed over and first coming back, per task.
    ///
    /// The only latency the journal can honestly report. It is wall clock time including
    /// whatever the worker was waiting on, not model time, and it is named accordingly.
    pub fn handback_seconds(&self) -> Vec<u64> {
        self.tasks
            .iter()
            .filter(|t| t.attempts > 0)
            .map(TaskTrail::elapsed)
            .collect()
    }
}

/// Folds a list of records.
pub fn fold(records: &[Record]) -> Folded {
    let mut by_task: BTreeMap<TaskId, TaskTrail> = BTreeMap::new();
    let mut out = Folded {
        records: records.len(),
        ..Folded::default()
    };

    for record in records {
        out.last_seq = Some(record.seq);
        out.last_at = Some(record.at);

        if let Event::Refused { what, why } = &record.event {
            out.refusals.push((record.at, what.clone(), why.clone()));
        }

        let Some(id) = record.event.task() else {
            continue;
        };
        let trail = by_task.entry(id.clone()).or_insert_with(|| TaskTrail {
            id: id.clone(),
            goal: None,
            owner: None,
            status: None,
            attempts: 0,
            rejections: 0,
            accepted: false,
            emergency: false,
            first_at: record.at,
            last_at: record.at,
        });
        trail.last_at = record.at;

        match &record.event {
            Event::Delegated { to, goal, .. } => {
                trail.owner = Some(to.clone());
                trail.goal = Some(goal.clone());
                // A handover is the start of the clock even when an earlier event mentioned
                // the task, because that is when somebody became responsible for it.
                trail.first_at = record.at;
            }
            Event::Moved { to, .. } => trail.status = Some(to.clone()),
            Event::Submitted { attempt, .. } => trail.attempts = trail.attempts.max(*attempt),
            Event::Reviewed { accepted, .. } => {
                if *accepted {
                    trail.accepted = true;
                } else {
                    trail.rejections += 1;
                }
            }
            Event::EmergencyDeclared { .. } => trail.emergency = true,
            _ => {}
        }
    }

    out.tasks = by_task.into_values().collect();
    out
}

/// Reads and folds the journal at `path`, counting what could not be read.
///
/// `event::read` skips a corrupt line rather than failing, which is the right behaviour and
/// also means a caller never learns it happened. Counting the difference between lines in the
/// file and records parsed is how the panel can say the record has a hole in it.
pub fn fold_file(path: &std::path::Path) -> Folded {
    let records = event::read(path).unwrap_or_default();
    let mut folded = fold(&records);

    if let Ok(text) = std::fs::read_to_string(path) {
        let lines = text.lines().filter(|l| !l.trim().is_empty()).count();
        folded.unreadable_lines = lines.saturating_sub(records.len());
    }
    folded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::army::Status;
    use crate::army::event::Journal;

    fn task() -> TaskId {
        TaskId::quoted("task-1")
    }

    fn written(events: &[(u64, &str, Event)]) -> (Folded, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("run/events.jsonl");
        let mut j = Journal::open(&path).unwrap();
        for (_, actor, e) in events {
            j.append(actor, e.clone()).unwrap();
        }
        drop(j);
        (fold_file(&path), d)
    }

    #[test]
    fn a_task_that_was_delegated_and_accepted_reads_as_healthy() {
        let (folded, _d) = written(&[
            (
                1,
                "mason",
                Event::Delegated {
                    task: task(),
                    to: "nora".into(),
                    goal: "make the balancer symmetric".into(),
                    parent: None,
                    must: vec!["it works".into()],
                    project: None,
                    workspace: None,
                    objective: None,
                },
            ),
            (
                2,
                "nora",
                Event::moved(&task(), Status::Assigned, Status::InHand),
            ),
            (
                3,
                "nora",
                Event::Submitted {
                    task: task(),
                    attempt: 1,
                    words: 40,
                },
            ),
            (
                4,
                "mason",
                Event::Reviewed {
                    task: task(),
                    accepted: true,
                    why: "good".into(),
                },
            ),
            (
                5,
                "mason",
                Event::moved(&task(), Status::Submitted, Status::Accepted),
            ),
        ]);

        let t = &folded.tasks[0];
        assert_eq!(t.owner.as_deref(), Some("nora"));
        assert_eq!(t.goal.as_deref(), Some("make the balancer symmetric"));
        assert_eq!(t.status.as_deref(), Some("accepted"));
        assert_eq!(t.health(), Health::Healthy);
        assert!(t.settled());
        assert!(folded.active().is_empty());
    }

    /// The distinction the four state model exists for. Sent back once is not stuck.
    #[test]
    fn a_task_sent_back_once_is_degraded_and_still_moving() {
        let (folded, _d) = written(&[
            (
                1,
                "mason",
                Event::Delegated {
                    task: task(),
                    to: "nora".into(),
                    goal: "g".into(),
                    parent: None,
                    must: vec!["it works".into()],
                    project: None,
                    workspace: None,
                    objective: None,
                },
            ),
            (
                2,
                "nora",
                Event::Submitted {
                    task: task(),
                    attempt: 1,
                    words: 10,
                },
            ),
            (
                3,
                "mason",
                Event::Reviewed {
                    task: task(),
                    accepted: false,
                    why: "no".into(),
                },
            ),
            (
                4,
                "mason",
                Event::moved(&task(), Status::Submitted, Status::ChangesRequested),
            ),
        ]);

        let t = &folded.tasks[0];
        assert_eq!(t.health(), Health::Degraded);
        assert!(!t.must_escalate(), "one attempt spent of three");
        assert_eq!(t.rejections, 1);
        assert_eq!(folded.blocked().len(), 0);
        assert_eq!(folded.active().len(), 1);
    }

    /// Out of attempts is blocked, which is a different colour and a different action.
    #[test]
    fn a_task_out_of_attempts_is_blocked_rather_than_merely_degraded() {
        let mut events = vec![(
            1,
            "mason",
            Event::Delegated {
                task: task(),
                to: "nora".into(),
                goal: "g".into(),
                parent: None,
                must: vec!["it works".into()],
                project: None,
                workspace: None,
                objective: None,
            },
        )];
        for attempt in 1..=MAX_ATTEMPTS {
            events.push((
                0,
                "nora",
                Event::Submitted {
                    task: task(),
                    attempt,
                    words: 10,
                },
            ));
            events.push((
                0,
                "mason",
                Event::Reviewed {
                    task: task(),
                    accepted: false,
                    why: "still no".into(),
                },
            ));
        }
        events.push((
            0,
            "mason",
            Event::moved(&task(), Status::Submitted, Status::ChangesRequested),
        ));

        let (folded, _d) = written(&events);
        let t = &folded.tasks[0];
        assert_eq!(t.attempts, MAX_ATTEMPTS);
        assert!(t.must_escalate());
        assert_eq!(t.health(), Health::Blocked);
        assert_eq!(folded.blocked().len(), 1);
    }

    #[test]
    fn an_abandoned_task_is_the_only_kind_that_failed() {
        let (folded, _d) = written(&[
            (
                1,
                "mason",
                Event::Delegated {
                    task: task(),
                    to: "nora".into(),
                    goal: "g".into(),
                    parent: None,
                    must: vec!["it works".into()],
                    project: None,
                    workspace: None,
                    objective: None,
                },
            ),
            (
                2,
                "mason",
                Event::moved(&task(), Status::InHand, Status::Abandoned),
            ),
        ]);
        assert_eq!(folded.tasks[0].health(), Health::Failed);
        assert_eq!(folded.failed().len(), 1);
    }

    #[test]
    fn a_refusal_is_kept_because_being_stopped_is_worth_recording() {
        let (folded, _d) = written(&[(
            1,
            "nora",
            Event::Refused {
                what: "delegate to carl".into(),
                why: "not downward".into(),
            },
        )]);
        assert_eq!(folded.refusals.len(), 1);
        assert_eq!(folded.refusals[0].1, "delegate to carl");
        assert!(folded.tasks.is_empty(), "a refusal names no task");
    }

    /// A journal with a hole in it should say so rather than quietly returning fewer events.
    #[test]
    fn a_corrupt_line_is_counted_rather_than_silently_skipped() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("events.jsonl");
        let mut j = Journal::open(&path).unwrap();
        j.append(
            "mason",
            Event::Decided {
                task: None,
                what: "start".into(),
            },
        )
        .unwrap();
        drop(j);

        let good = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, format!("{good}{{ not json\n")).unwrap();

        let folded = fold_file(&path);
        assert_eq!(folded.records, 1);
        assert_eq!(folded.unreadable_lines, 1, "the hole is visible");
    }

    #[test]
    fn a_journal_that_does_not_exist_folds_to_nothing_rather_than_failing() {
        let folded = fold_file(std::path::Path::new("/nonexistent/events.jsonl"));
        assert_eq!(folded.records, 0);
        assert_eq!(folded.unreadable_lines, 0);
        assert_eq!(folded.last_seq, None);
        assert!(folded.tasks.is_empty());
    }

    /// A task nobody has moved yet has no status, and guessing one would be inventing state.
    #[test]
    fn a_task_with_no_movement_has_unknown_health() {
        let (folded, _d) = written(&[(
            1,
            "mason",
            Event::Delegated {
                task: task(),
                to: "nora".into(),
                goal: "g".into(),
                parent: None,
                must: vec!["it works".into()],
                project: None,
                workspace: None,
                objective: None,
            },
        )]);
        assert_eq!(folded.tasks[0].status, None);
        assert_eq!(folded.tasks[0].health(), Health::Unknown);
    }
}
