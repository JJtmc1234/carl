//! What changed recently, gathered from every record the panel holds.
//!
//! Four different things get written down by four different parts of the backend: journal
//! records, handovers, milestones and turns of the conversation. Somebody standing in front of
//! the screen does not care which file a thing came out of, they care what happened last, so
//! they are merged into one list and sorted by when.
//!
//! Nothing here invents a beat. An empty army produces an empty list, and the screen says so
//! rather than filling the space with anything.

use eframe::egui::Color32;

use carl::army::event::{Event, Intervention};

use crate::model::{Snapshot, Speaker};
use crate::theme;

/// One thing that happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Beat {
    pub at: u64,
    /// The small caps word on the left, which says what kind of thing this was.
    pub kind: &'static str,
    pub who: String,
    pub what: String,
    pub color: Color32,
}

/// Everything that happened, newest first, capped.
///
/// Ties are broken by keeping the order the sources were read in, which is stable frame to
/// frame. Rows that swap places under the pointer are worse than rows in a slightly arbitrary
/// order, and two things stamped with the same second genuinely have no order to recover.
pub fn recent(snapshot: &Snapshot, limit: usize) -> Vec<Beat> {
    let mut out: Vec<Beat> = Vec::new();

    for record in &snapshot.events {
        out.push(Beat {
            at: record.at,
            kind: kind_of(&record.event),
            who: record.actor.clone(),
            what: describe(&record.event),
            color: colour_of(&record.event),
        });
    }

    for d in &snapshot.delegations {
        out.push(Beat {
            at: d.at,
            kind: "HANDED DOWN",
            who: d.from.clone(),
            what: format!("gave {} {}", d.to, d.goal),
            color: theme::COLD,
        });
    }

    for p in &snapshot.projects {
        for m in &p.milestones {
            out.push(Beat {
                at: m.at,
                kind: "MILESTONE",
                who: p.project.name.clone(),
                what: m.title.clone(),
                color: theme::GOOD,
            });
        }
    }

    for turn in &snapshot.conversation {
        out.push(Beat {
            at: turn.at,
            kind: match turn.from {
                Speaker::Jj => "JJ SAID",
                Speaker::Carl => "CARL SAID",
            },
            who: match turn.from {
                Speaker::Jj => "jj".into(),
                Speaker::Carl => "carl".into(),
            },
            what: one_line(&turn.text),
            color: match turn.from {
                Speaker::Jj => theme::INTERVENE,
                Speaker::Carl => theme::ACCENT,
            },
        });
    }

    // Stable, so two beats stamped the same second keep the order their sources were read in
    // rather than shuffling every frame.
    // Newest first, so the most recent thing the army did is the first line read.
    out.sort_by_key(|e| std::cmp::Reverse(e.at));
    out.truncate(limit);
    out
}

/// The word for a record, in the panel's own vocabulary rather than the journal's.
fn kind_of(event: &Event) -> &'static str {
    match event {
        Event::Delegated { .. } => "HANDED DOWN",
        Event::Moved { .. } => "MOVED",
        Event::Submitted { .. } => "SUBMITTED",
        Event::Reviewed { accepted: true, .. } => "ACCEPTED",
        Event::Reviewed { .. } => "SENT BACK",
        Event::Refused { .. } => "REFUSED",
        Event::EmergencyDeclared { .. } => "EMERGENCY",
        Event::Decided { .. } => "DECIDED",
        Event::Intervened { .. } => "JJ ACTED",
        Event::Notified { .. } => "TOLD",
    }
}

/// What a record says, in one line.
pub fn describe(event: &Event) -> String {
    match event {
        Event::Delegated { to, goal, .. } => format!("gave {to} {goal}"),
        Event::Moved { from, to, .. } => format!("task moved from {from} to {to}"),
        Event::Submitted { attempt, words, .. } => {
            format!("attempt {attempt}, {words} words")
        }
        Event::Reviewed { accepted, why, .. } => {
            let head = if *accepted { "accepted" } else { "sent back" };
            format!("{head}. {why}")
        }
        Event::Refused { what, why } => format!("{what}. {why}"),
        Event::EmergencyDeclared { why, .. } => format!("emergency declared. {why}"),
        Event::Decided { what, .. } => what.clone(),
        Event::Intervened { what } => describe_intervention(what),
        Event::Notified { who, about } => format!("told {who} about record {about}"),
    }
}

fn describe_intervention(what: &Intervention) -> String {
    match what {
        Intervention::Message { to, what } => format!("messaged {to} directly. {what}"),
        Intervention::Objective { what } => format!("new objective. {what}"),
        Intervention::Answered { answer, .. } => format!("answered. {answer}"),
        Intervention::Stopped { why, .. } => format!("stopped a task. {why}"),
        Intervention::Replaced { goal, why, .. } if why.is_empty() => {
            format!("replaced a task with {goal}")
        }
        Intervention::Replaced { goal, why, .. } => format!("replaced a task with {goal}. {why}"),
        Intervention::Override { agent, instruction } => {
            format!("standing instruction to {agent}. {instruction}")
        }
    }
}

/// The one hue a beat carries, which is always about what kind of thing happened and never
/// about making the feed look busy.
fn colour_of(event: &Event) -> Color32 {
    match event {
        Event::Reviewed { accepted: true, .. } => theme::GOOD,
        Event::Reviewed { .. } | Event::Refused { .. } => theme::WARN,
        Event::EmergencyDeclared { .. } => theme::BAD,
        Event::Intervened { .. } => theme::INTERVENE,
        _ => theme::COLD,
    }
}

/// A turn of a conversation collapsed onto one line, for the feed.
fn one_line(text: &str) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(96) {
        Some((cut, _)) => format!("{}...", flat[..cut].trim_end()),
        None => flat,
    }
}

#[cfg(test)]
mod tests;
