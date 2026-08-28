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
use carl::panel::view::TaskView;

// Process 3's types, used as they are rather than copied.
//
// The panel used to define its own `Diagnostic` and `Project`, and both flattened a
// distinction the collectors are careful to keep: a metric that cannot say "unreadable" turns
// an unmeasurable disk and a full one into the same number, and a bare timestamp forces an age
// onto army state that has none. Re exporting keeps both distinctions all the way to the
// screen, and means there is one definition of each rather than two that drift.
pub use carl::providers::health::{Diagnostic, Health, Kind, Metric, Reading};
pub use carl::providers::projects::{Achievement, Milestone, Project, ProjectView, Status};

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
    pub task: Option<String>,
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

/// A tool call Carl is holding still, waiting for JJ.
///
/// Its own type rather than a `Decision`, even though both are questions with two buttons. A
/// decision is identified by the journal sequence that raised it and is answered by writing to
/// the journal. This is identified by a string the hook minted, is answered over a different
/// channel, and is not a thing that happened to the army until JJ says so. Sharing a type would
/// mean sharing an id field, and the decision path parses that id as a number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    /// Minted by the hook. Opaque, and never parsed.
    pub id: String,
    /// As the CLI names it: `Bash`, `Write`, `Read`.
    pub tool: String,
    /// The part worth reading before deciding: the command, or the path.
    pub detail: String,
    /// Which surface asked, so JJ can see whether this came from him or from Slack.
    pub surface: String,
    pub asked_at: u64,
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
    pub task: Option<String>,
}

/// Everything the panel knows at one instant.
///
/// Not `Eq`, because a diagnostic can carry a float. Comparing readings for exact equality is
/// not something the screen should be doing anyway.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    pub agents: Vec<AgentView>,
    /// The backend's own task view, used as it is.
    ///
    /// Not rebuilt into an `army::task::Task`. Constructing one would mean running
    /// `Task::assign`, which mints a fresh id and re-checks a delegation that already happened,
    /// so the panel would be drawing a task the army never issued.
    pub tasks: Vec<TaskView>,
    pub projects: Vec<ProjectView>,
    pub diagnostics: Vec<Diagnostic>,
    pub conversation: Vec<Turn>,
    pub decisions: Vec<Decision>,
    /// Tool calls waiting on JJ right now.
    ///
    /// Not part of the backend snapshot and deliberately so. A question exists only while a
    /// process is holding still for it, so it is pushed and withdrawn on the live stream rather
    /// than being a thing a snapshot could resurrect after it had already timed out.
    pub permissions: Vec<Permission>,
    pub delegations: Vec<Delegation>,
    pub events: Vec<carl::army::event::Record>,
    /// Unix seconds the snapshot was taken.
    pub at: u64,
    /// The journal sequence this was built from.
    ///
    /// Carried so a resync can join the stream at exactly the point the backend built it,
    /// rather than the screen keeping a number of its own that could drift.
    pub seq_at: u64,
}

impl Snapshot {
    pub fn agent(&self, name: &str) -> Option<&AgentView> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn task(&self, id: &str) -> Option<&TaskView> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn project(&self, name: &str) -> Option<&ProjectView> {
        self.projects.iter().find(|p| p.project.name == name)
    }

    /// Every task the backend says belongs to a project.
    ///
    /// The link is `TaskView::project`, which the record now carries. Nothing is matched by
    /// name or guessed at from a goal, so a project with no tasks shows none rather than the
    /// ones that happen to read like it.
    pub fn tasks_in(&self, project: &carl::ProjectId) -> Vec<&TaskView> {
        self.tasks
            .iter()
            .filter(|t| t.project.as_ref() == Some(project))
            .collect()
    }

    /// Everything recorded about one task, newest last.
    pub fn events_about(&self, id: &str) -> Vec<&carl::army::event::Record> {
        self.events
            .iter()
            .filter(|r| r.event.task().is_some_and(|t| t.as_str() == id))
            .collect()
    }
}

mod render;

pub use render::{STALE_AFTER, age_secs, board_of, stale};
