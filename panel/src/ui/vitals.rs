//! The one paragraph summary of the army, worked out once and drawn in three places.
//!
//! The rail wants it, the top strip wants a piece of it, and the overview is mostly it. Three
//! screens counting blocked agents three different ways is how a panel starts contradicting
//! itself, so the counting happens here and is tested here.
//!
//! Two rules shape all of it.
//!
//! **JJ is not an agent.** He sits outside the operational army, so he is never in a count of
//! who is working, idle or unknown. Counting him made the idle number wrong by one forever.
//!
//! **Unknown is counted as unknown.** Never folded into healthy to make a headline look
//! better, never folded into failed to make it look urgent. It is its own number and it is
//! shown.

use eframe::egui::Color32;

use crate::model::{AgentStatus, Health, Snapshot};
use crate::theme;

use super::widgets::{self, Mark};

/// Everything the summary needs, counted once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Vitals {
    pub working: usize,
    pub review: usize,
    pub blocked: usize,
    pub idle: usize,
    pub unknown: usize,
    pub decisions: usize,
    pub failed: usize,
    pub degraded: usize,
    pub held: usize,
    pub unmeasured: usize,
    pub healthy: usize,
    pub projects_active: usize,
    pub projects_blocked: usize,
}

impl Vitals {
    /// Everybody in the operational army, which is everybody except JJ.
    pub fn agents(&self) -> usize {
        self.working + self.review + self.blocked + self.idle + self.unknown
    }

    pub fn components(&self) -> usize {
        self.failed + self.degraded + self.held + self.unmeasured + self.healthy
    }

    /// The worst health anything is in, which is what the headline reports.
    pub fn worst(&self) -> Health {
        if self.failed > 0 {
            Health::Failed
        } else if self.held > 0 || self.blocked > 0 {
            Health::Blocked
        } else if self.degraded > 0 {
            Health::Degraded
        } else if self.components() == 0 {
            Health::Unknown
        } else {
            Health::Healthy
        }
    }

    /// The headline, as a word, a colour and a shape. Never as a colour alone.
    ///
    /// The wording is deliberately about what is true rather than about a grade. "ALL CLEAR"
    /// would be a claim about the whole system; "NOTHING REPORTING A FAULT" is a claim about
    /// what was measured, which is the only claim the panel is entitled to make.
    pub fn headline(&self) -> (&'static str, Color32, Mark) {
        let worst = self.worst();
        let word = match worst {
            Health::Failed => "SOMETHING HAS FAILED",
            Health::Blocked => "SOMETHING IS HELD UP",
            Health::Degraded => "RUNNING DEGRADED",
            Health::Unknown => "NOTHING HAS BEEN MEASURED",
            Health::Healthy => "NOTHING REPORTING A FAULT",
        };
        (
            word,
            widgets::health_color(worst),
            widgets::health_mark(worst),
        )
    }

    /// How many things want JJ personally, which is what the rail badges.
    pub fn wants_jj(&self) -> usize {
        self.decisions + self.blocked + self.failed + self.projects_blocked
    }
}

/// Counts everything the summary reports.
pub fn read(snapshot: &Snapshot) -> Vitals {
    let mut v = Vitals {
        decisions: snapshot.decisions.len(),
        ..Default::default()
    };

    for agent in &snapshot.agents {
        if is_human(&agent.name) {
            continue;
        }
        match agent.status {
            AgentStatus::Working => v.working += 1,
            AgentStatus::AwaitingReview => v.review += 1,
            AgentStatus::Blocked => v.blocked += 1,
            AgentStatus::Idle => v.idle += 1,
            AgentStatus::Unknown => v.unknown += 1,
        }
    }

    for d in &snapshot.diagnostics {
        match d.health {
            Health::Failed => v.failed += 1,
            Health::Degraded => v.degraded += 1,
            Health::Blocked => v.held += 1,
            Health::Unknown => v.unmeasured += 1,
            Health::Healthy => v.healthy += 1,
        }
    }

    for p in &snapshot.projects {
        if p.project.status == crate::model::Status::Active {
            v.projects_active += 1;
        }
        if !p.project.blockers.is_empty() {
            v.projects_blocked += 1;
        }
    }

    v
}

/// Whether a name belongs to a person rather than to an agent.
///
/// Asked of `army::org` rather than compared against the string "jj", so a second human joining
/// the organisation is handled by the table that already knows about them.
pub fn is_human(name: &str) -> bool {
    carl::army::org::find(name).is_some_and(|a| a.rank == carl::army::org::Rank::Human)
}

/// One thing that wants somebody, in the words the screen uses for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Need {
    /// What kind of thing it is, for the small caps label on the left.
    pub kind: &'static str,
    /// The subject: an agent, a component, a project.
    pub subject: String,
    /// What is actually wrong, in one line.
    pub detail: String,
    /// Where clicking it should take somebody.
    pub goes_to: crate::app::Tab,
    pub color: Color32,
    pub mark: Mark,
}

/// Everything that wants JJ, worst first.
///
/// The order is a judgement and it is written down so it can be argued with. A question Carl
/// asked outranks everything, because it is the only item on the list that is already waiting
/// on a person. A failed component outranks a blocked agent, because a blocked agent usually
/// has a cause further down the list and clearing the cause clears both.
pub fn needs(snapshot: &Snapshot) -> Vec<Need> {
    let mut out = Vec::new();

    for d in &snapshot.decisions {
        out.push(Need {
            kind: "CARL ASKS",
            subject: "carl".into(),
            detail: d.question.clone(),
            goes_to: crate::app::Tab::Carl,
            color: theme::ACCENT,
            mark: Mark::Half,
        });
    }
    for d in snapshot
        .diagnostics
        .iter()
        .filter(|d| d.health == Health::Failed)
    {
        out.push(Need {
            kind: "FAILED",
            subject: d.component.clone(),
            detail: d.summary.clone(),
            goes_to: crate::app::Tab::Diagnostics,
            color: theme::BAD,
            mark: Mark::Cross,
        });
    }
    for a in snapshot
        .agents
        .iter()
        .filter(|a| a.status == AgentStatus::Blocked)
    {
        out.push(Need {
            kind: "BLOCKED",
            subject: a.name.clone(),
            detail: a
                .blocker
                .clone()
                .unwrap_or_else(|| "blocked, and no reason was recorded".into()),
            goes_to: crate::app::Tab::Agents,
            color: theme::BAD,
            mark: Mark::Barred,
        });
    }
    for p in snapshot
        .projects
        .iter()
        .filter(|p| !p.project.blockers.is_empty())
    {
        out.push(Need {
            kind: "PROJECT HELD",
            subject: p.project.name.clone(),
            detail: p.project.blockers.join(". "),
            goes_to: crate::app::Tab::Projects,
            color: theme::WARN,
            mark: Mark::Barred,
        });
    }
    for d in snapshot
        .diagnostics
        .iter()
        .filter(|d| matches!(d.health, Health::Blocked | Health::Degraded))
    {
        out.push(Need {
            kind: widgets::health_label(d.health),
            subject: d.component.clone(),
            detail: d.summary.clone(),
            goes_to: crate::app::Tab::Diagnostics,
            color: widgets::health_color(d.health),
            mark: widgets::health_mark(d.health),
        });
    }

    out
}

#[cfg(test)]
mod tests;
