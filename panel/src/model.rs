//! What the panel draws, which is a projection of the army and never a second copy of it.
//!
//! The rule that shapes this file: anything the army already defines is used as it is. An
//! agent is `carl::army::org::Agent`, a task is `carl::army::task::Task`, an event is
//! `carl::army::event::Record`. This module adds only the things the army genuinely has no
//! type for, which are the live overlay on an agent, a project, and a diagnostic reading.
//!
//! Two definitions of `Task` is the failure this whole army spent a day avoiding, so the panel
//! is not going to introduce a third.
//!
//! **Unknown is a value.** Every field the backend may not know is an `Option`, and the UI
//! draws the absence rather than a zero. A diagnostic with no reading says so. A worker with
//! no task says so. Nothing here invents a plausible number, because a panel that quietly
//! shows a made up figure is worse than one that shows a gap.

use carl::army::org::{Agent, Rank};
use carl::army::task::{Task, TaskId};

/// How the panel is currently getting its data.
///
/// Shown at all times and never guessed at. A panel that looks live while disconnected is the
/// one failure that makes every other number on screen a lie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    Live,
    /// Trying to get back, with how many goes it has had.
    Connecting {
        attempt: u32,
    },
    Disconnected {
        why: String,
    },
}

impl Link {
    pub fn is_live(&self) -> bool {
        matches!(self, Link::Live)
    }

    pub fn label(&self) -> String {
        match self {
            Link::Live => "LINK LIVE".into(),
            Link::Connecting { attempt } => format!("RECONNECTING {attempt}"),
            Link::Disconnected { .. } => "LINK LOST".into(),
        }
    }
}

/// What one agent is doing right now.
///
/// The static half comes from `org::Agent` and is borrowed rather than copied. Only the live
/// half lives here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentView {
    /// The name in `org`, which is the join key for everything else.
    pub name: String,
    pub department: Option<String>,
    pub sub_department: Option<String>,
    pub status: AgentStatus,
    /// The task it holds, if it holds one.
    pub task: Option<TaskId>,
    pub blocker: Option<String>,
    /// What it last actually did, in its own words, and when.
    pub last_activity: Option<String>,
    pub last_activity_at: Option<u64>,
    /// Which model is configured, when personnel knows.
    pub model: Option<String>,
    /// Whether a process is up. `None` when nothing has measured it.
    pub process: Option<ProcessState>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
}

impl AgentView {
    /// A view for an agent nothing is known about yet, which is the honest starting state.
    pub fn unknown(name: &str) -> Self {
        Self {
            name: name.to_string(),
            department: None,
            sub_department: None,
            status: AgentStatus::Unknown,
            task: None,
            blocker: None,
            last_activity: None,
            last_activity_at: None,
            model: None,
            process: None,
            worktree: None,
            branch: None,
        }
    }

    pub fn agent(&self) -> Option<&'static Agent> {
        carl::army::org::find(&self.name)
    }

    pub fn rank(&self) -> Option<Rank> {
        self.agent().map(|a| a.rank)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    /// Nothing to do, and that is fine.
    Idle,
    Working,
    /// Waiting on a review by whoever assigned the task.
    AwaitingReview,
    /// Stopped by something it cannot clear itself.
    Blocked,
    /// Nothing has told the panel anything about this agent.
    Unknown,
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Idle => "IDLE",
            AgentStatus::Working => "WORKING",
            AgentStatus::AwaitingReview => "REVIEW",
            AgentStatus::Blocked => "BLOCKED",
            AgentStatus::Unknown => "UNKNOWN",
        }
    }

    /// Whether this is worth the eye going to it first.
    pub fn wants_attention(self) -> bool {
        matches!(self, AgentStatus::Blocked)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Stopped,
}

/// Something Carl cannot settle on his own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub id: String,
    pub asked_at: u64,
    pub question: String,
    pub detail: Option<String>,
    /// What JJ can pick. Free text is always allowed as well.
    pub options: Vec<String>,
}

/// One turn in the conversation with Carl.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub at: u64,
    pub from: Speaker,
    pub text: String,
    /// True while the text is still arriving.
    pub streaming: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speaker {
    Jj,
    Carl,
}

/// A handover, for the short list of what Carl has been doing with what JJ asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    pub at: u64,
    pub from: String,
    pub to: String,
    pub goal: String,
    pub task: Option<TaskId>,
}

/// Everything the panel knows at one instant.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub agents: Vec<AgentView>,
    pub tasks: Vec<Task>,
    pub projects: Vec<Project>,
    pub diagnostics: Vec<Diagnostic>,
    pub conversation: Vec<Turn>,
    pub decisions: Vec<Decision>,
    pub delegations: Vec<Delegation>,
    pub events: Vec<carl::army::event::Record>,
    /// Unix seconds the snapshot was taken.
    pub at: u64,
}

impl Snapshot {
    pub fn agent(&self, name: &str) -> Option<&AgentView> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn task(&self, id: &TaskId) -> Option<&Task> {
        self.tasks.iter().find(|t| &t.id == id)
    }

    pub fn project(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// Everything recorded about one task, newest last.
    pub fn events_about(&self, id: &TaskId) -> Vec<&carl::army::event::Record> {
        self.events
            .iter()
            .filter(|r| r.event.task() == Some(id))
            .collect()
    }
}

mod diagnostic;
mod project;

pub use diagnostic::{Diagnostic, Health, Reading};
pub use project::{Milestone, Phase, Project};
