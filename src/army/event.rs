//! What happened, in a form somebody can ask questions of later.
//!
//! Append only, one JSON object per line, and the same rule the rest of Carl already follows:
//! write it down before acting on it. A crash after recording loses the outcome, which can be
//! looked up. A crash before recording loses the fact that anything was attempted, and nothing
//! recovers that.
//!
//! The reason this exists rather than logging is that a hierarchy is only worth having if
//! somebody can check it was followed. Twenty agents producing work with no record is twenty
//! opinions and a story about where they came from. With a record, "who approved this", "how
//! many times did she try", and "did anybody actually review it" are questions with answers.
//!
//! Refusals are recorded as well as actions, and they are the interesting half. A log holding
//! only what happened cannot answer what somebody tried to do and was stopped from doing,
//! which is the question worth asking when something has gone wrong.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::task::{Status, TaskId};
use crate::Result;

/// Something worth being able to ask about later.
///
/// Deliberately closed rather than a free string. A string means every writer invents its own
/// wording and no reader can count anything, and counting is most of what a record is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Work handed from one agent to a direct report.
    ///
    /// Carries the parent and the verification conditions because a task is never written to
    /// disk anywhere else. This line is the only durable evidence the task exists, so anything
    /// a reader needs in order to rebuild it has to be here or it is gone when the process ends.
    /// Both fields default, so lines written before they existed still read.
    Delegated {
        task: TaskId,
        to: String,
        goal: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<TaskId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        must: Vec<String>,
    },
    /// A task moved from one state to another.
    Moved {
        task: TaskId,
        from: String,
        to: String,
    },
    /// An agent produced something for its task.
    Submitted {
        task: TaskId,
        attempt: u32,
        words: usize,
    },
    /// A review decided.
    Reviewed {
        task: TaskId,
        accepted: bool,
        why: String,
    },
    /// Something was refused, and by what rule.
    ///
    /// The interesting half of the record. Without it nobody can tell a rule that is working
    /// from a rule nothing has ever hit.
    Refused { what: String, why: String },
    /// A lead was allowed to implement, which normally it may not.
    ///
    /// Recorded separately from everything else because it is the one exception to the rank
    /// rules, and an exception nobody can count is an exception that becomes the habit.
    EmergencyDeclared { task: TaskId, why: String },
    /// A department or the chief said what it decided, having read what came back.
    Decided { task: Option<TaskId>, what: String },
    /// JJ reached past the chain of command and did something himself.
    ///
    /// Its own variant rather than a normal event with `actor: "jj"`, because the difference
    /// between "Mason reassigned Nora's task" and "JJ reassigned Nora's task over Mason's head"
    /// is the whole point of having a chain. Recording the second as though it were the first
    /// would make the record lie about who decided, which is the one thing it exists to answer.
    ///
    /// JJ has absolute authority, so this is never refused. It is only ever made visible.
    Intervened { what: Intervention },
    /// Somebody was told about something they did not do.
    ///
    /// Points at the sequence number of what they are being told about rather than repeating it,
    /// so a notification can never come to disagree with the thing it notifies about.
    Notified { who: String, about: u64 },
}

/// What JJ did, in a form a reader can count rather than parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intervention", rename_all = "snake_case")]
pub enum Intervention {
    /// A message straight to one agent, going around its lead.
    Message { to: String, what: String },
    /// A new objective for Carl. The ordinary way in, and still recorded.
    Objective { what: String },
    /// An answer to something Carl asked JJ to decide.
    Answered { question: String, answer: String },
    /// A task stopped where it stands.
    Stopped { task: TaskId, why: String },
    /// A task stopped and replaced with a different goal.
    Replaced {
        task: TaskId,
        goal: String,
        why: String,
    },
    /// A standing instruction to one agent that overrides what its lead told it.
    Override { agent: String, instruction: String },
}

impl Intervention {
    /// The agent this reached past the chain to touch, when there is one.
    pub fn agent(&self) -> Option<&str> {
        match self {
            Intervention::Message { to, .. } => Some(to),
            Intervention::Override { agent, .. } => Some(agent),
            Intervention::Objective { .. } | Intervention::Answered { .. } => None,
            Intervention::Stopped { .. } | Intervention::Replaced { .. } => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Intervention::Message { .. } => "message",
            Intervention::Objective { .. } => "objective",
            Intervention::Answered { .. } => "answered",
            Intervention::Stopped { .. } => "stopped",
            Intervention::Replaced { .. } => "replaced",
            Intervention::Override { .. } => "override",
        }
    }
}

impl Event {
    /// A short name, for counting without matching every variant.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Delegated { .. } => "delegated",
            Event::Moved { .. } => "moved",
            Event::Submitted { .. } => "submitted",
            Event::Reviewed { .. } => "reviewed",
            Event::Refused { .. } => "refused",
            Event::EmergencyDeclared { .. } => "emergency_declared",
            Event::Decided { .. } => "decided",
            Event::Intervened { .. } => "intervened",
            Event::Notified { .. } => "notified",
        }
    }

    /// The task this concerns, when it concerns one.
    pub fn task(&self) -> Option<&TaskId> {
        match self {
            Event::Delegated { task, .. }
            | Event::Moved { task, .. }
            | Event::Submitted { task, .. }
            | Event::Reviewed { task, .. }
            | Event::EmergencyDeclared { task, .. } => Some(task),
            Event::Decided { task, .. } => task.as_ref(),
            Event::Intervened { what } => match what {
                Intervention::Stopped { task, .. } | Intervention::Replaced { task, .. } => {
                    Some(task)
                }
                _ => None,
            },
            Event::Refused { .. } | Event::Notified { .. } => None,
        }
    }

    /// Made from a status change, so the two cannot describe it differently.
    pub fn moved(task: &TaskId, from: Status, to: Status) -> Self {
        Event::Moved {
            task: task.clone(),
            from: from.to_string(),
            to: to.to_string(),
        }
    }
}

/// One line of the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub seq: u64,
    /// Unix seconds.
    pub at: u64,
    /// Who did it. An agent name, always.
    pub actor: String,
    #[serde(flatten)]
    pub event: Event,
}

/// The record itself.
pub struct Journal {
    path: PathBuf,
    next_seq: u64,
}

impl Journal {
    /// Opens the record, continuing the numbering where it left off.
    ///
    /// Reading the whole file to find the last sequence number is fine at this size and wrong
    /// at a million lines, and it is written down here so whoever hits that knows it was a
    /// choice.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let next_seq = read(&path)?.last().map_or(1, |r| r.seq + 1);
        Ok(Self { path, next_seq })
    }

    /// Writes one line, and flushes it before returning.
    ///
    /// Flushed rather than buffered, because the whole value of this file is that it survives
    /// whatever happens next.
    pub fn append(&mut self, actor: &str, event: Event) -> Result<Record> {
        let record = Record {
            seq: self.next_seq,
            at: now(),
            actor: actor.to_string(),
            event,
        };

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
        file.flush()?;

        self.next_seq += 1;
        Ok(record)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Everything recorded, in order.
///
/// A line that cannot be read is skipped rather than fatal. A record with one corrupt line is
/// still worth reading, and refusing to open it would lose everything else in it.
pub fn read(path: impl AsRef<Path>) -> Result<Vec<Record>> {
    let text = match std::fs::read_to_string(path.as_ref()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Everything about one task, in order.
pub fn about(records: &[Record], task: &TaskId) -> Vec<Record> {
    records
        .iter()
        .filter(|r| r.event.task() == Some(task))
        .cloned()
        .collect()
}

/// Unix seconds. `pub(crate)` because the chain writes the same clock into agent folders, and
/// two clocks would let a journal entry and a state file disagree about when something happened.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (Journal, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(d.path().join("run/events.jsonl")).unwrap();
        (j, d)
    }

    #[test]
    fn what_is_written_can_be_read_back() {
        let (mut j, _d) = journal();
        let task = TaskId::quoted("abc123");

        j.append(
            "mason",
            Event::Delegated {
                task: task.clone(),
                to: "nora".into(),
                goal: "fix the counter".into(),
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].actor, "mason");
        assert_eq!(back[0].seq, 1);
        assert_eq!(back[0].event.kind(), "delegated");
    }

    #[test]
    fn numbering_continues_across_reopening() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("events.jsonl");

        let mut first = Journal::open(&path).unwrap();
        first
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "go".into(),
                },
            )
            .unwrap();
        drop(first);

        let mut second = Journal::open(&path).unwrap();
        let r = second
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "again".into(),
                },
            )
            .unwrap();

        assert_eq!(r.seq, 2, "a restart must not renumber from one");
    }

    /// The interesting half. Without it nobody can tell a rule that is working from a rule
    /// nothing has ever hit.
    #[test]
    fn refusals_are_recorded_too() {
        let (mut j, _d) = journal();
        j.append(
            "carl",
            Event::Refused {
                what: "delegate to nora".into(),
                why: "not a direct report".into(),
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back[0].event.kind(), "refused");
        assert_eq!(back[0].event.task(), None);
    }

    /// An exception nobody can count is an exception that becomes the habit.
    #[test]
    fn an_emergency_is_its_own_kind_of_event() {
        let (mut j, _d) = journal();
        let task = TaskId::quoted("t1");
        j.append(
            "mason",
            Event::EmergencyDeclared {
                task: task.clone(),
                why: "the build is broken and nora is not available".into(),
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back[0].event.kind(), "emergency_declared");
        assert_eq!(back[0].event.task(), Some(&task));
    }

    /// The two must not be able to describe the same change differently.
    #[test]
    fn a_move_is_built_from_the_statuses_themselves() {
        let e = Event::moved(&TaskId::quoted("t"), Status::Submitted, Status::Accepted);
        match e {
            Event::Moved { from, to, .. } => {
                assert_eq!(from, "submitted");
                assert_eq!(to, "accepted");
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn everything_about_one_task_can_be_pulled_out() {
        let (mut j, _d) = journal();
        let mine = TaskId::quoted("mine");
        let other = TaskId::quoted("other");

        j.append(
            "nora",
            Event::Submitted {
                task: mine.clone(),
                attempt: 1,
                words: 200,
            },
        )
        .unwrap();
        j.append(
            "nora",
            Event::Submitted {
                task: other.clone(),
                attempt: 1,
                words: 10,
            },
        )
        .unwrap();
        j.append(
            "mason",
            Event::Reviewed {
                task: mine.clone(),
                accepted: true,
                why: "good".into(),
            },
        )
        .unwrap();

        let story = about(&read(j.path()).unwrap(), &mine);
        assert_eq!(story.len(), 2);
        assert!(story.iter().all(|r| r.event.task() == Some(&mine)));
    }

    /// A record with one bad line is still worth reading, and refusing to open it would lose
    /// everything else.
    #[test]
    fn one_corrupt_line_does_not_lose_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("events.jsonl");

        let mut j = Journal::open(&path).unwrap();
        j.append(
            "carl",
            Event::Decided {
                task: None,
                what: "one".into(),
            },
        )
        .unwrap();

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "this is not json at all").unwrap();
        drop(f);

        let mut j = Journal::open(&path).unwrap();
        j.append(
            "carl",
            Event::Decided {
                task: None,
                what: "two".into(),
            },
        )
        .unwrap();

        let back = read(&path).unwrap();
        assert_eq!(back.len(), 2, "the good lines survive: {back:?}");
    }

    #[test]
    fn a_record_that_does_not_exist_yet_reads_as_empty() {
        assert!(
            read("/definitely/not/here/events.jsonl")
                .unwrap()
                .is_empty()
        );
    }
}
