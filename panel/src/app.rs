//! Everything the panel knows and everything it decides, with no drawing in it.
//!
//! The split is deliberate and it is what makes this testable. `App` holds the state and all
//! the rules. The `ui` modules read it and draw it and put input back through these methods.
//! Nothing in here touches egui, so every requirement worth checking can be checked without a
//! window and without a screenshot.
//!
//! Three rules live here rather than in the drawing.
//!
//! **A reconnect replaces the world.** Events missed while disconnected are gone, so patching
//! the next ones onto what was on screen leaves a state nobody ever sent. Coming back takes a
//! fresh snapshot.
//!
//! **Hiding keeps everything.** Toggling away and back is not a restart. Tab, selections, the
//! open workspace and the half typed message all survive, because a panel that forgets where
//! you were is one you stop toggling.
//!
//! **Nothing is applied locally.** A command goes out and the screen changes when the backend
//! says so.

use std::time::{Duration, Instant};

use crate::command::{Command, InterventionKind, WorkspaceRequest};
use crate::model::{Link, Snapshot, Speaker, Turn};
use crate::source::PanelDataSource;

mod apply;
mod intervene;

#[cfg(test)]
mod tests;

/// The four principal tabs. The editor and the terminal are not among them on purpose: they
/// are tools opened from something, not places to go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Carl,
    Agents,
    Diagnostics,
    Projects,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Carl, Tab::Agents, Tab::Diagnostics, Tab::Projects];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Carl => "CARL",
            Tab::Agents => "AGENTS",
            Tab::Diagnostics => "DIAGNOSTICS",
            Tab::Projects => "PROJECTS",
        }
    }

    /// One line under the name, so the sidebar says what each is for.
    pub fn caption(self) -> &'static str {
        match self {
            Tab::Carl => "command",
            Tab::Agents => "who is doing what",
            Tab::Diagnostics => "health",
            Tab::Projects => "work",
        }
    }
}

/// What the contextual workspace is showing, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub open: WorkspaceRequest,
    /// Set once Process 3 can fill it. Until then the pane says so rather than faking content.
    pub content: Option<String>,
}

/// The state that has to survive being hidden.
///
/// Its own struct so the test for that can assert on the whole thing rather than on a list of
/// fields somebody will forget to extend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub tab: Tab,
    pub agent: Option<String>,
    pub project: Option<String>,
    pub workspace: Option<Workspace>,
    pub draft: String,
    pub objective: String,
    pub conversation_at_end: bool,
}

pub struct App {
    source: Box<dyn PanelDataSource>,
    pub snapshot: Snapshot,
    pub link: Link,

    pub tab: Tab,
    pub agent: Option<String>,
    pub project: Option<String>,
    pub workspace: Option<Workspace>,

    /// What JJ has typed and not sent.
    pub draft: String,
    pub objective: String,
    /// Whether the conversation is pinned to the bottom.
    pub conversation_at_end: bool,

    /// False while the panel is hidden. Nothing is thrown away when it is.
    pub visible: bool,

    /// The intervention being composed, if any.
    pub intervening: Option<Intervening>,

    /// What the last submit said, shown briefly.
    pub notice: Option<(String, bool)>,

    /// Agents whose row changed recently, so the change can be shown landing rather than just
    /// being different the next time you look.
    pub lit: Vec<(String, Instant)>,
    /// Set when a reconnect has just replaced the world, so the panel can say so.
    pub resynced_at: Option<Instant>,
}

/// An intervention part way through being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intervening {
    pub agent: String,
    pub kind: InterventionKind,
    pub body: String,
    /// True once JJ has asked to send something forceful and been asked to confirm.
    pub confirming: bool,
}

/// How long a changed row stays lit.
pub const LIT_FOR: Duration = Duration::from_millis(2200);

impl App {
    pub fn new(mut source: Box<dyn PanelDataSource>) -> Self {
        let snapshot = source.snapshot();
        let link = source.link();
        Self {
            source,
            snapshot,
            link,
            tab: Tab::Carl,
            agent: None,
            project: None,
            workspace: None,
            draft: String::new(),
            objective: String::new(),
            conversation_at_end: true,
            visible: true,
            intervening: None,
            notice: None,
            lit: Vec::new(),
            resynced_at: None,
        }
    }

    pub fn source_name(&self) -> String {
        self.source.describe()
    }

    /// Takes everything waiting and applies it. Called every frame, and by tests directly.
    pub fn tick(&mut self) {
        let events = self.source.poll();
        for event in events {
            self.apply(event);
        }
        self.lit.retain(|(_, at)| at.elapsed() < LIT_FOR);
    }

    /// Sends something, and records what came back. Applies nothing itself.
    ///
    /// Refused here first when the panel knows the link is down, rather than left to the
    /// source. The source refuses too, but by then the panel has already believed it might
    /// work, and the thing JJ needs is to be told before pressing anything that an
    /// intervention cannot reach an agent right now.
    pub fn submit(&mut self, command: Command) {
        if !self.link.is_live() {
            self.notice = Some((
                format!("not sent, {}", self.link.label().to_lowercase()),
                false,
            ));
            return;
        }
        match self.source.submit(command) {
            Ok(()) => self.notice = Some(("sent".into(), true)),
            Err(why) => self.notice = Some((why, false)),
        }
    }

    /// Whether anything can be sent at all, so the composer and the intervention buttons can
    /// show that they cannot rather than failing when pressed.
    pub fn can_send(&self) -> bool {
        self.link.is_live()
    }

    /// Sends whatever JJ has typed to Carl, if it is worth sending.
    pub fn send_draft(&mut self) {
        let text = self.draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.draft.clear();
        self.conversation_at_end = true;
        self.submit(Command::SayToCarl(text));
    }

    /// Sends a new objective, which is a different act from a message and is sent as one.
    pub fn send_objective(&mut self) {
        let goal = self.objective.trim().to_string();
        if goal.is_empty() {
            return;
        }
        self.objective.clear();
        self.submit(Command::SetObjective(goal));
    }

    pub fn answer_decision(&mut self, id: &str, answer: &str) {
        if answer.trim().is_empty() {
            return;
        }
        self.submit(Command::AnswerDecision {
            id: id.to_string(),
            answer: answer.trim().to_string(),
        });
    }

    pub fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    /// Picks an agent, which is also what opens the detail view.
    pub fn select_agent(&mut self, name: &str) {
        self.agent = Some(name.to_string());
        // A part written intervention belongs to whoever it was being written to.
        if self.intervening.as_ref().is_some_and(|i| i.agent != name) {
            self.intervening = None;
        }
    }

    pub fn select_project(&mut self, name: &str) {
        self.project = Some(name.to_string());
    }

    pub fn open_workspace(&mut self, request: WorkspaceRequest) {
        if request == WorkspaceRequest::Close {
            self.workspace = None;
            return;
        }
        self.workspace = Some(Workspace {
            open: request,
            content: None,
        });
    }

    pub fn close_workspace(&mut self) {
        self.workspace = None;
    }

    /// Hides or shows the whole panel, keeping everything either way.
    pub fn toggle_visible(&mut self) {
        self.visible = !self.visible;
    }

    /// What must survive a hide, gathered so the test can assert on all of it at once.
    pub fn kept(&self) -> Kept {
        Kept {
            tab: self.tab,
            agent: self.agent.clone(),
            project: self.project.clone(),
            workspace: self.workspace.clone(),
            draft: self.draft.clone(),
            objective: self.objective.clone(),
            conversation_at_end: self.conversation_at_end,
        }
    }

    /// Whether an agent row should be shown as having just changed.
    pub fn is_lit(&self, name: &str) -> bool {
        self.lit.iter().any(|(n, _)| n == name)
    }

    /// The turn currently being streamed, if one is.
    pub fn streaming_turn(&self) -> Option<&Turn> {
        self.snapshot
            .conversation
            .last()
            .filter(|t| t.streaming && t.from == Speaker::Carl)
    }
}
