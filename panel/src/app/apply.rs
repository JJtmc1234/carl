//! Folding one event into what is on screen.
//!
//! Two decisions in here are the ones that keep the panel truthful.
//!
//! **Coming back replaces the world.** Whatever happened while the link was down was not
//! delivered, so the next event cannot simply be laid on top of the old state. Reconnecting
//! takes a fresh snapshot and throws away what was there, which is the only version anybody
//! actually sent.
//!
//! **Streaming appends until it does not.** Carl's answer arrives in pieces. Each piece extends
//! the turn in place, and the piece marked finished is what turns the caret off. Treating every
//! piece as a new turn is how one answer becomes eight paragraphs.

use std::time::Instant;

use crate::model::{Link, Speaker, Turn};
use crate::source::PanelEvent;

use super::App;

impl App {
    pub(super) fn apply(&mut self, event: PanelEvent) {
        match event {
            PanelEvent::LinkChanged(link) => self.relink(link),

            // Fresh truth, replacing rather than merging. The conversation is carried across
            // because it is this session's talking and the backend never held it.
            PanelEvent::Resynced(fresh) => {
                self.last_seq = self.last_seq.max(fresh.seq_at);
                let talking = std::mem::take(&mut self.snapshot.conversation);
                self.snapshot = *fresh;
                if self.snapshot.conversation.is_empty() {
                    self.snapshot.conversation = talking;
                }
                self.resynced_at = Some(Instant::now());
                self.lit.clear();
            }

            PanelEvent::CommandRefused(why) => self.notice = Some((why, false)),

            // Diagnostics and nothing else. Not an event, not a record, and it does not move
            // anything the army's ordering depends on.
            PanelEvent::TelemetryChanged { at, diagnostics } => {
                for reading in diagnostics {
                    match self
                        .snapshot
                        .diagnostics
                        .iter_mut()
                        .find(|d| d.component == reading.component)
                    {
                        Some(slot) => *slot = reading,
                        None => self.snapshot.diagnostics.push(reading),
                    }
                }
                self.sampled_at = Some(at);
            }

            PanelEvent::AgentChanged(view) => {
                let name = view.name.clone();
                match self.snapshot.agents.iter_mut().find(|a| a.name == name) {
                    Some(slot) => *slot = *view,
                    None => self.snapshot.agents.push(*view),
                }
                self.light(&name);
            }

            PanelEvent::TaskChanged(task) => {
                match self.snapshot.tasks.iter_mut().find(|t| t.id == task.id) {
                    Some(slot) => *slot = *task,
                    None => self.snapshot.tasks.push(*task),
                }
            }

            PanelEvent::Recorded(record) => {
                // The record's own sequence, so the screen agrees with the journal rather than
                // counting for itself.
                self.last_seq = self.last_seq.max(record.seq);
                self.snapshot.events.push(*record);
                // Bounded, because the panel shows the recent past and the journal is the
                // place the whole of it lives.
                let len = self.snapshot.events.len();
                if len > KEEP_EVENTS {
                    self.snapshot.events.drain(..len - KEEP_EVENTS);
                }
            }

            PanelEvent::JjSaid(text) => self.snapshot.conversation.push(Turn {
                at: self.snapshot.at,
                from: Speaker::Jj,
                text,
                streaming: false,
            }),

            PanelEvent::CarlSaid { text, streaming } => self.carl_said(&text, streaming),

            PanelEvent::DecisionRaised(decision) => {
                let id = decision.id.clone();
                self.snapshot.decisions.retain(|d| d.id != id);
                self.snapshot.decisions.push(*decision);
            }

            PanelEvent::DecisionSettled { id } => {
                self.snapshot.decisions.retain(|d| d.id != id);
            }

            // Replaced rather than pushed, so the backlog a fresh subscriber is sent cannot put
            // the same question on screen twice.
            PanelEvent::PermissionAsked(request) => {
                let id = request.id.clone();
                self.snapshot.permissions.retain(|p| p.id != id);
                self.snapshot.permissions.push(*request);
            }

            // Off the screen whoever ended it, including when nobody did and it timed out. A
            // question left showing after the process behind it gave up is a button that does
            // nothing, which is worse than no button.
            // Off the screen, and say so.
            //
            // Removing the row silently is why the button read as broken. JJ pressed Allow, the
            // answer landed, the tool call went through, and the band still had a question in
            // it because the army had asked another one in the meantime. Nothing on screen
            // distinguished "your click worked" from "your click did nothing", so the working
            // case and the broken case looked identical.
            PanelEvent::PermissionSettled { id, allowed } => {
                let tool = self
                    .snapshot
                    .permissions
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.tool.clone());
                self.snapshot.permissions.retain(|p| p.id != id);
                if let Some(tool) = tool {
                    self.notice = Some((
                        format!("{} {tool}", if allowed { "allowed" } else { "refused" }),
                        allowed,
                    ));
                    // And in the band itself, which is where JJ was looking when he pressed it.
                    self.just_settled
                        .push((tool, allowed, std::time::Instant::now()));
                }
            }

            PanelEvent::Delegated(delegation) => {
                self.snapshot.delegations.push(*delegation);
                let len = self.snapshot.delegations.len();
                if len > KEEP_DELEGATIONS {
                    self.snapshot.delegations.drain(..len - KEEP_DELEGATIONS);
                }
            }

            PanelEvent::DiagnosticChanged(d) => {
                match self
                    .snapshot
                    .diagnostics
                    .iter_mut()
                    .find(|x| x.component == d.component)
                {
                    Some(slot) => *slot = *d,
                    None => self.snapshot.diagnostics.push(*d),
                }
            }

            PanelEvent::ProjectChanged(p) => {
                match self
                    .snapshot
                    .projects
                    .iter_mut()
                    .find(|x| x.project.id == p.project.id)
                {
                    Some(slot) => *slot = *p,
                    None => self.snapshot.projects.push(*p),
                }
            }

            PanelEvent::MilestoneReached { project, milestone } => {
                if let Some(p) = self
                    .snapshot
                    .projects
                    .iter_mut()
                    .find(|p| p.project.name == project)
                {
                    // Newest first, which is the order the provider keeps them in and the
                    // order the pane draws.
                    p.milestones.insert(0, *milestone);
                }
            }
        }
    }

    /// The link changed, which is the one event that can invalidate everything else.
    fn relink(&mut self, link: Link) {
        let was_live = self.link.is_live();
        let now_live = link.is_live();
        self.link = link;

        if now_live && !was_live {
            // Back after a gap. The army half of the screen predates the gap and nothing filled
            // it, so it is replaced rather than continued.
            //
            // The conversation and the event list are not the army half. The backend keeps
            // neither: a snapshot carries no turns and no records, so what comes back has both
            // empty, and assigning it wholesale wipes them. They are this session's own history,
            // built from frames as they arrived, and there is nowhere to fetch them from again.
            // Blanking them would punish JJ for a dropped socket by deleting what he just said
            // and everything he had watched happen.
            let talking = std::mem::take(&mut self.snapshot.conversation);
            let watched = std::mem::take(&mut self.snapshot.events);
            self.snapshot = self.source.snapshot();
            self.snapshot.conversation = talking;
            self.snapshot.events = watched;
            self.resynced_at = Some(Instant::now());
            self.lit.clear();
        }
    }

    /// Carl's answer, arriving in pieces.
    fn carl_said(&mut self, text: &str, streaming: bool) {
        let extend = self
            .snapshot
            .conversation
            .last()
            .is_some_and(|t| t.from == Speaker::Carl && t.streaming);

        if extend {
            let turn = self.snapshot.conversation.last_mut().expect("just checked");
            turn.text.push_str(text);
            turn.streaming = streaming;
        } else {
            self.snapshot.conversation.push(Turn {
                at: self.snapshot.at,
                from: Speaker::Carl,
                text: text.to_string(),
                streaming,
            });
        }
        self.conversation_at_end = true;
    }

    /// Marks a row as just changed, so the change can be seen landing.
    fn light(&mut self, name: &str) {
        self.lit.retain(|(n, _)| n != name);
        self.lit.push((name.to_string(), Instant::now()));
    }
}

/// How much recent history the panel keeps. The journal has all of it.
const KEEP_EVENTS: usize = 200;
const KEEP_DELEGATIONS: usize = 40;
