//! What the panel is allowed to know, and how it says it does not know.
//!
//! Every type here is a **projection**. Nothing in this file is a source of truth, and nothing
//! here is ever written to disk. `org` says who exists, the personnel folders say what each
//! agent holds, and the journal says what happened. A view is those three read at one moment
//! and shaped for a screen.
//!
//! That is the whole reason the panel is safe to add. A second store would drift from the army
//! and then somebody would have to decide which was right. A projection cannot drift, because
//! it is thrown away and rebuilt rather than updated.
//!
//! **Unknown is a value here, not an absence.** A panel that shows a blank where it means "no
//! process is running" and the same blank where it means "nobody has looked" is worse than one
//! that shows nothing, because the blank reads as an answer. So state that has not been
//! measured says so.

use serde::{Deserialize, Serialize};

use crate::ProjectId;
use crate::army::org::Rank;

/// Something the panel has not been told, distinguished from something it has been told is
/// absent.
///
/// `Known(None)` means asked and there is none. `Unknown` means nobody has looked. A screen can
/// render those differently, and rendering them the same is how a dead process looks idle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "known", rename_all = "snake_case")]
pub enum Maybe<T> {
    Unknown,
    Known { value: T },
}

impl<T> Maybe<T> {
    pub fn known(value: T) -> Self {
        Maybe::Known { value }
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Maybe::Unknown)
    }
}

impl<T> From<Option<T>> for Maybe<T> {
    /// Only for values where `None` genuinely means nobody looked.
    fn from(v: Option<T>) -> Self {
        match v {
            Some(value) => Maybe::Known { value },
            None => Maybe::Unknown,
        }
    }
}

// `Health` used to be defined here, and so did `DiagnosticView`, `Metric` and `ProjectView`.
// They are Process 3's now, re-exported rather than reimplemented.
//
// Not tidiness. The panel's own versions flattened two distinctions the collectors are careful
// to keep. A metric held an `f64`, which cannot say "unreadable", so an unmeasurable disk and a
// full disk arrived on screen as the same number. And `measured_at` was a bare `u64`, which
// forced a timestamp onto army state that has none, because it is true until something changes
// it rather than true at an instant. Re-exporting keeps both distinctions all the way to the
// wire.
pub use crate::providers::health::{Diagnostic as DiagnosticView, Health, Kind, Metric, Reading};
pub use crate::providers::projects::ProjectView;

/// One agent, as the panel sees them.
///
/// Name, display, rank, remit and `reports_to` come from the compiled table and are therefore
/// never unknown and never wrong. Everything below them comes from a folder or from the record,
/// and any of it can be genuinely absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentView {
    /// Lowercase, the identifier used in events, tasks and folder names.
    pub name: String,
    pub display: String,
    pub rank: Rank,
    pub remit: String,
    pub reports_to: Option<String>,
    /// From the agent's profile. Absent for the chief, who owns all of them.
    pub department: Option<String>,
    pub sub_department: Option<String>,
    /// True when this agent has a folder on disk. False means enlisted in the table and not yet
    /// founded, which is a real and different state from having an empty folder.
    pub enlisted: bool,
    /// The task the folder says they are holding.
    pub holding: Option<String>,
    /// The status of that task, as the journal last reported it.
    pub task_status: Maybe<String>,
    /// True when the task has been sent back and is on its last allowed attempt.
    pub blocked: Maybe<bool>,
    /// The most recent journal record naming this agent as actor, in machine form.
    pub last_event: Maybe<LastEvent>,
    /// Which model the folder says this agent runs on.
    pub model: Maybe<String>,
    /// Whether a process for this agent is running right now.
    ///
    /// Always `Unknown` in v1. Nothing measures it yet, and saying `Known(false)` would be a
    /// claim nobody checked. Process 3's providers fill this in.
    pub process: Maybe<ProcessState>,
}

/// The last thing an agent did, kept structured so a screen can count rather than parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastEvent {
    pub seq: u64,
    pub at: u64,
    pub kind: String,
    pub task: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Running,
    Exited,
}

/// One task, rebuilt from the record.
///
/// A task is never written to disk anywhere. It lives in memory for the length of a chain run,
/// and the journal is the only thing that outlives it, so every field here was folded out of
/// journal records rather than read from a task store. There is no task store, on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskView {
    pub id: String,
    pub goal: String,
    pub owner: String,
    /// Who assigned it, and who therefore reviews it.
    pub assigner: String,
    pub parent: Option<String>,
    /// Which project this task belongs to, from the record.
    ///
    /// `None` for a task nobody put in one, and for every task delegated before projects
    /// existed. Those are the same on the wire and mean the same thing: nobody said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
    pub status: String,
    pub attempts: u32,
    /// What must be observable before it is done. Empty only for a task delegated before the
    /// record carried them.
    pub must: Vec<String>,
    pub review: Maybe<Review>,
    /// When it was delegated, in unix seconds. Always known, because delegation is what creates
    /// a task in the record.
    pub delegated_at: u64,
    /// When anything last happened to it.
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    pub accepted: bool,
    pub why: String,
    pub by: String,
    pub at: u64,
}

/// Carl himself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarlView {
    /// Whether a conversation is open with him right now.
    pub status: Maybe<ProcessState>,
    /// Things Carl has put to JJ and not had answered.
    pub pending: Vec<Pending>,
    /// What JJ has asked for that is not finished.
    pub objectives: Vec<String>,
    /// The most recent handovers, newest last.
    pub recent_delegations: Vec<TaskView>,
}

/// Something waiting on JJ.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// The journal sequence that raised it, which is how an answer is tied back to its question.
    pub seq: u64,
    pub at: u64,
    pub asked_by: String,
    pub question: String,
    pub task: Option<String>,
}

/// Everything, at one moment.
///
/// `seq` is the journal sequence this was built from, and it is the join between the snapshot
/// and the stream. A panel takes the snapshot, then subscribes from that exact sequence, and
/// what it renders is continuous with no gap and no repeat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelSnapshot {
    pub seq: u64,
    pub at: u64,
    pub carl: CarlView,
    pub agents: Vec<AgentView>,
    pub tasks: Vec<TaskView>,
    pub projects: Vec<ProjectView>,
    pub diagnostics: Vec<DiagnosticView>,
}
