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
//! The vocabulary is here. The file it is written to is in `journal`, which is where the
//! locking, the sequence numbering and the reading live, because the question "what may be
//! recorded" and the question "how does a line get safely onto the end of a file" have nothing
//! to say to each other.
//!
//! Refusals are recorded as well as actions, and they are the interesting half. A log holding
//! only what happened cannot answer what somebody tried to do and was stopped from doing,
//! which is the question worth asking when something has gone wrong.

mod journal;

#[cfg(test)]
mod tests;

pub(crate) use journal::now;
pub use journal::{Journal, about, read};

use serde::{Deserialize, Serialize};

use super::task::{Status, TaskId};
use crate::ProjectId;

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
        /// Which project the work belongs to, when it belongs to one.
        ///
        /// Defaults, so every line written before projects existed still reads, as `None`. An
        /// old journal is not a broken journal, and refusing to open one would throw away the
        /// only record of everything that happened before this field did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<ProjectId>,
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
