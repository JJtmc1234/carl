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

pub use workspace::{Comparison, Pane, Workspace};

mod apply;
mod intervene;
mod workspace;

#[cfg(test)]
mod tests;

/// The five principal tabs. The editor and the terminal are not among them on purpose: they
/// are tools opened from something, not places to go.
///
/// Overview was added in the redesign and is where the panel opens. The four that follow each
/// answer one question in depth, and none of them could answer the question somebody actually
/// has when they walk up to a screen, which is whether anything is wrong. Overview does only
/// that, and every line on it is a way into one of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Overview,
    Carl,
    Agents,
    Diagnostics,
    Projects,
}

impl Tab {
    pub const ALL: [Tab; 5] = [
        Tab::Overview,
        Tab::Carl,
        Tab::Agents,
        Tab::Diagnostics,
        Tab::Projects,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Overview => "OVERVIEW",
            Tab::Carl => "CARL",
            Tab::Agents => "AGENTS",
            Tab::Diagnostics => "DIAGNOSTICS",
            Tab::Projects => "PROJECTS",
        }
    }

    /// One line under the name, so the sidebar says what each is for.
    pub fn caption(self) -> &'static str {
        match self {
            Tab::Overview => "the whole army at once",
            Tab::Carl => "command",
            Tab::Agents => "who is doing what",
            Tab::Diagnostics => "health",
            Tab::Projects => "work",
        }
    }
}

/// The state that has to survive being hidden.
///
/// Its own struct so the test for that can assert on the whole thing rather than on a list of
/// fields somebody will forget to extend.
///
/// Not `Eq`, because an open investigation carries a reading that can be a float.
#[derive(Debug, Clone, PartialEq)]
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
    /// Process 3's facade. Every terminal, file and comparison goes through it, and nothing in
    /// the interface below `app` ever touches a process or a path.
    service: carl::providers::workspace::service::Workspace,
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
    /// The journal sequence the screen is caught up to.
    ///
    /// Moved by an army event and by a resync, never by telemetry, which has no sequence.
    pub last_seq: u64,
    /// When the machine was last sampled, from telemetry.
    ///
    /// Kept apart from anything to do with the journal. This moves when a number was measured
    /// again, which is a different clock from the army doing something.
    pub sampled_at: Option<u64>,
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
            service: carl::providers::workspace::service::Workspace::new(),
            snapshot,
            link,
            tab: Tab::Overview,
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
            last_seq: 0,
            sampled_at: None,
        }
    }

    pub fn source_name(&self) -> String {
        self.source.describe()
    }

    /// The journal sequence the screen is caught up to, for saying so on screen and in a run.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Takes everything waiting and applies it. Called every frame, and by tests directly.
    pub fn tick(&mut self) {
        let events = self.source.poll();
        for event in events {
            self.apply(event);
        }
        self.pump_workspace();
        self.sweep_workspace();
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

    /// Opens something in the pane, through the facade.
    ///
    /// Whatever was open is closed first, so a pane cannot leak a pty or a file handle by
    /// being replaced.
    pub fn open_workspace(&mut self, request: WorkspaceRequest) {
        if request == WorkspaceRequest::Close {
            self.close_workspace();
            return;
        }
        self.close_workspace();

        let mut opened = workspace::open(&mut self.service, request.clone(), 24, 100);

        // An investigation is a lookup into the diagnostics already on screen. The component
        // string is never turned into a path or a command, here or in the facade.
        if let WorkspaceRequest::Investigate { component } = &request {
            match self
                .service
                .investigate(&self.diagnostics_snapshot(), component)
            {
                Some(found) => opened.pane = Pane::Investigating(Box::new(found)),
                None => {
                    opened.trouble = Some(format!("nothing on the boards is called {component}"))
                }
            }
        }
        self.workspace = Some(opened);
    }

    /// The diagnostics the boards are drawing, in the shape the facade looks things up in.
    fn diagnostics_snapshot(&self) -> carl::providers::diagnostics::Snapshot {
        carl::providers::diagnostics::Snapshot {
            army: self
                .snapshot
                .diagnostics
                .iter()
                .filter(|d| d.group() == "army")
                .cloned()
                .collect(),
            machine: self
                .snapshot
                .diagnostics
                .iter()
                .filter(|d| d.group() == "system")
                .cloned()
                .collect(),
        }
    }

    /// Closes the pane and lets the facade release whatever was behind it.
    pub fn close_workspace(&mut self) {
        if let Some(open) = self.workspace.take()
            && let Some(id) = open.session
        {
            let _ = match open.pane {
                Pane::Terminal { .. } => self.service.close_terminal(id),
                Pane::Editor { .. } => self.service.close_file(id),
                _ => Ok(()),
            };
        }
    }

    /// Releases dead sessions that nothing is showing.
    ///
    /// Only when no pane is open, and that condition is the whole point. `reap` collects every
    /// dead terminal, so calling it while a pane is displaying one would take away the
    /// scrollback at the exact moment somebody wanted to read why the shell went. A dead shell
    /// on screen is kept until it is dismissed, and this is for anything left behind after that.
    pub fn sweep_workspace(&mut self) {
        if self.workspace.is_some() {
            return;
        }
        let _ = self.service.reap();
    }

    /// How many terminals and files the facade is holding, for proving nothing leaks.
    pub fn held(&self) -> (usize, usize) {
        self.service.held()
    }

    /// Reads whatever the terminal has said and notices when it has gone.
    ///
    /// Called every frame. `drain` never blocks, which is what makes that safe.
    pub fn pump_workspace(&mut self) {
        let Some(open) = self.workspace.as_mut() else {
            return;
        };
        let Some(id) = open.session else {
            return;
        };
        if let Pane::Terminal {
            output,
            alive,
            exited,
            ..
        } = &mut open.pane
        {
            // Drained first, so whatever the shell said on its way out is kept. Draining after
            // noticing it had gone would lose the last thing it printed, which is usually the
            // reason it went.
            if let Ok(bytes) = self.service.drain(id)
                && !bytes.is_empty()
            {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }

            *alive = self.service.is_alive(id);
            if !*alive && !*exited {
                *exited = true;
                // Deliberately not closed here. The scrollback is the evidence and it stays
                // readable until the pane is dismissed, which is what releases the session.
                output.push_str("\n[the shell exited]\n");
            }
        }
    }

    /// Sends a line to the terminal, which is the only way the interface talks to a process.
    pub fn terminal_send(&mut self) {
        let Some(open) = self.workspace.as_mut() else {
            return;
        };
        let Some(id) = open.session else {
            return;
        };
        if let Pane::Terminal { input, .. } = &mut open.pane {
            let line = std::mem::take(input);
            let _ = self.service.input_line(id, &line);
        }
    }

    /// Tells the pty how big the pane is now.
    pub fn terminal_resize(&mut self, rows: u16, cols: u16) {
        let Some(open) = self.workspace.as_ref() else {
            return;
        };
        let Some(id) = open.session else {
            return;
        };
        if matches!(open.pane, Pane::Terminal { .. }) {
            let _ = self.service.resize(
                id,
                carl::providers::workspace::terminal::Size { rows, cols },
            );
        }
    }

    /// Saves the editor buffer, and shows a refusal rather than swallowing it.
    pub fn editor_save(&mut self) {
        let Some(id) = self.workspace.as_ref().and_then(|w| w.session) else {
            return;
        };
        // Asked before saving, so a file that moved underneath is known about rather than
        // discovered from the shape of an error message.
        let moved = self.service.changed_on_disk(id).unwrap_or(false);

        let Some(open) = self.workspace.as_mut() else {
            return;
        };
        if let Pane::Editor {
            buffer,
            refused,
            conflict,
            changed_on_disk,
            ..
        } = &mut open.pane
        {
            match self.service.save(id, buffer) {
                Ok(()) => {
                    *refused = None;
                    *conflict = false;
                    *changed_on_disk = false;
                }
                // Nothing is overwritten and the buffer is untouched. A read only file has
                // nothing to resolve, and a file that moved underneath has a choice to make,
                // so the two are told apart rather than sharing one message.
                Err(e) => {
                    *refused = Some(e.to_string());
                    *conflict = moved;
                    *changed_on_disk = moved;
                }
            }
        }
    }

    /// Takes what is on disk, discarding the buffer.
    pub fn editor_reload(&mut self) {
        let Some(open) = self.workspace.as_mut() else {
            return;
        };
        let Some(id) = open.session else {
            return;
        };
        if let Pane::Editor {
            buffer,
            refused,
            changed_on_disk,
            conflict,
            ..
        } = &mut open.pane
        {
            match self.service.reload(id) {
                Ok(()) => {
                    *buffer = self.service.contents(id).unwrap_or_default().to_string();
                    *refused = None;
                    *changed_on_disk = false;
                    *conflict = false;
                }
                Err(e) => *refused = Some(e.to_string()),
            }
        }
    }

    /// Whether the file has moved under the editor since it was opened.
    pub fn editor_check_disk(&mut self) {
        let Some(open) = self.workspace.as_mut() else {
            return;
        };
        let Some(id) = open.session else {
            return;
        };
        if let Pane::Editor {
            changed_on_disk, ..
        } = &mut open.pane
        {
            *changed_on_disk = self.service.changed_on_disk(id).unwrap_or(false);
        }
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
