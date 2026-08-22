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

use super::personnel::{AgentId, Hours};
use super::runtime::{Continuity, Session};
use super::task::{Status, TaskId};
use crate::{ProjectId, SessionId};

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
        /// Where the work may happen, when a lead said. Defaults, so an older line still reads.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace: Option<String>,
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
    /// A process was started for an agent, and what survived into it.
    ///
    /// The supervisor is the only writer of this and the four below it. They are the only events
    /// in the vocabulary that are about a process rather than about work, and keeping them here
    /// rather than in a second log is deliberate: "the worker crashed and then the task was
    /// reported finished" is a sentence somebody needs to be able to read in order, and two files
    /// cannot be read in order.
    AgentStarted {
        agent: AgentId,
        /// What the agent was called at the time, for whoever reads the file. The id is the
        /// identifier, and nothing decides anything from this.
        name: String,
        continuity: Continuity,
        /// Consecutive failed starts before this one. Zero is the ordinary case.
        #[serde(default)]
        attempt: u32,
    },
    /// A process ended and nobody asked it to.
    ///
    /// Includes a process that went down with the supervisor that started it, which is not a
    /// fault in the agent but is still an end nobody requested. What separates this from
    /// `AgentStopped` is not how violent it was, it is whether anybody decided it.
    AgentCrashed {
        agent: AgentId,
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        /// Consecutive failed starts including this one.
        #[serde(default)]
        attempt: u32,
    },
    /// A start was attempted and there was no process at the end of it.
    ///
    /// Separate from a crash because nothing ran. A crash has a process that did something and
    /// then stopped, and reporting a binary that will not execute as one would send whoever is
    /// debugging it looking for a transcript that was never written.
    AgentStartFailed {
        agent: AgentId,
        name: String,
        why: String,
        #[serde(default)]
        attempt: u32,
    },
    /// The supervisor was told to keep no process for this agent.
    ///
    /// Says nothing about the agent's work, which is not the supervisor's to say.
    AgentStopped {
        agent: AgentId,
        name: String,
        why: String,
    },
    /// An agent was put down for the night by its own hours.
    ///
    /// Separate from `AgentStopped` because the two are undone by different things. A stop waits
    /// for somebody to decide and this one ends by itself, and counting "how often was an agent
    /// deliberately not running" gives a useless answer if a nightly event and a decision are
    /// the same row.
    ///
    /// There is no event for waking again. The next pass starts the process and writes
    /// `AgentStarted`, which already says everything a second record would, and two records of
    /// one fact are one failed write away from disagreeing.
    AgentSlept {
        agent: AgentId,
        name: String,
        /// The window, so a reader can see which arrangement put it down without going and
        /// finding a config file that may have changed since.
        hours: Hours,
    },
    /// The supervisor has stopped trying to start this agent.
    ///
    /// The end of the backoff, and the point at which a person has to decide something. Recorded
    /// as its own event because an agent nobody is trying to start looks exactly like an agent
    /// nobody has needed yet, and those are not the same.
    AgentGaveUp {
        agent: AgentId,
        name: String,
        why: String,
    },
    /// An agent came back with less than it had.
    ///
    /// Only written when something was actually lost, never on an ordinary start. `AgentStarted`
    /// already carries the continuity of every start, so writing this alongside each one would be
    /// a second record of the same fact, and two records of one fact are one failed write away
    /// from disagreeing. This is here for the case worth finding by searching: the conversation
    /// that could not be resumed, and which session was set aside over it.
    ContinuityChanged {
        agent: AgentId,
        name: String,
        from: Session,
        to: Session,
        why: String,
        /// The conversation that was given up on, kept so somebody can go and read it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        abandoned: Option<SessionId>,
    },
    /// A sleeping or stopped agent was asked for again, and what for.
    ///
    /// The reason is a value rather than a sentence, so there is no way to write down a wake
    /// meaning "something happened". An agent woken for nothing in particular is an agent
    /// nobody can tell why is running, and the cost of it is a model sitting there thinking.
    AgentWoken {
        agent: AgentId,
        name: String,
        because: Because,
    },
    /// A lead gave one of its people permission to do something, for one task.
    ///
    /// The grant is here and the enforcement is not. Recording that a lead allowed a worker to
    /// write in one directory is Carl's business. Stopping it writing anywhere else is the
    /// capability layer's, in a different process, which is the only place a boundary can be
    /// made to actually hold.
    ///
    /// Refusals are not a separate event, because `Refused` already records everything anybody
    /// was stopped from doing and splitting that in two would mean counting refusals twice.
    Granted {
        task: TaskId,
        to: String,
        what: String,
    },
    /// Somebody was told about something they did not do.
    ///
    /// Points at the sequence number of what they are being told about rather than repeating it,
    /// so a notification can never come to disagree with the thing it notifies about.
    Notified { who: String, about: u64 },
}

/// Why an agent was woken.
///
/// Deliberately without a variant meaning "in general". Every way of waking an agent names the
/// thing it is being woken for, so there is no way to express a wake that nobody could later
/// justify.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "because", rename_all = "snake_case")]
pub enum Because {
    /// There is a task waiting for it.
    Task { task: TaskId },
    /// Something has gone wrong that it is needed for.
    Incident { what: String },
    /// Whoever it reports to asked for it.
    Lead { who: String },
}

impl Because {
    pub fn kind(&self) -> &'static str {
        match self {
            Because::Task { .. } => "task",
            Because::Incident { .. } => "incident",
            Because::Lead { .. } => "lead",
        }
    }
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
            Event::AgentStarted { .. } => "agent_started",
            Event::AgentCrashed { .. } => "agent_crashed",
            Event::AgentStartFailed { .. } => "agent_start_failed",
            Event::AgentStopped { .. } => "agent_stopped",
            Event::AgentSlept { .. } => "agent_slept",
            Event::AgentGaveUp { .. } => "agent_gave_up",
            Event::ContinuityChanged { .. } => "continuity_changed",
            Event::AgentWoken { .. } => "agent_woken",
            Event::Granted { .. } => "granted",
        }
    }

    /// The agent whose process this is about, when it is about one.
    ///
    /// The runtime half of the vocabulary answers this and the work half answers `task`. An event
    /// answering neither is one about the organisation rather than about anybody in particular.
    pub fn agent(&self) -> Option<&AgentId> {
        match self {
            Event::AgentStarted { agent, .. }
            | Event::AgentCrashed { agent, .. }
            | Event::AgentStartFailed { agent, .. }
            | Event::AgentStopped { agent, .. }
            | Event::AgentSlept { agent, .. }
            | Event::AgentGaveUp { agent, .. }
            | Event::ContinuityChanged { agent, .. }
            | Event::AgentWoken { agent, .. } => Some(agent),
            _ => None,
        }
    }

    /// The task this concerns, when it concerns one.
    pub fn task(&self) -> Option<&TaskId> {
        match self {
            Event::Delegated { task, .. }
            | Event::Moved { task, .. }
            | Event::Submitted { task, .. }
            | Event::Reviewed { task, .. }
            | Event::EmergencyDeclared { task, .. }
            | Event::Granted { task, .. } => Some(task),
            Event::Decided { task, .. } => task.as_ref(),
            Event::Intervened { what } => match what {
                Intervention::Stopped { task, .. } | Intervention::Replaced { task, .. } => {
                    Some(task)
                }
                _ => None,
            },
            // The runtime events are about a process, not about work, which is the boundary the
            // whole supervisor is built on. An agent crashing says nothing about whether its task
            // succeeded, and letting one answer `task` here would be the first place that stopped
            // being true.
            Event::Refused { .. }
            | Event::Notified { .. }
            | Event::AgentStarted { .. }
            | Event::AgentCrashed { .. }
            | Event::AgentStartFailed { .. }
            | Event::AgentStopped { .. }
            | Event::AgentSlept { .. }
            | Event::AgentGaveUp { .. }
            | Event::AgentWoken { .. }
            | Event::ContinuityChanged { .. } => None,
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
    /// Who did it.
    ///
    /// An agent name, or `supervisor` for the runtime events, which are the only ones no agent
    /// performs. Attributing a crash to the agent it happened to would make "the last thing this
    /// agent did" answer with something the agent did not do.
    pub actor: String,
    #[serde(flatten)]
    pub event: Event,
}
