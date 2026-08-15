//! Commands going out, and journal events coming in.
//!
//! The outbound half is a straight naming exercise: the panel's `Command` says what JJ meant
//! and `PanelCommand` says what the backend does about it, and mapping the two in one place
//! means no widget ever builds a wire type.
//!
//! The inbound half is the interesting one. An event is a journal record, not a new view, so
//! it says what happened and not what everything looks like afterwards. Only what the record
//! definitively states is applied. A `moved` frame moves that task's status and nothing else,
//! because guessing at the rest is how a screen drifts away from the army it is describing and
//! stays confidently wrong until the next resync.

use carl::army::event::Event as JournalEvent;
use carl::panel::PanelCommand;
use carl::panel::wire::{Entity, PanelEvent as WireEvent};

use crate::command::{Command, InterventionKind};
use crate::model::{Diagnostic, Snapshot};
use crate::source::PanelEvent;

/// What the backend should be asked to do, or `None` when it is not the backend's business.
pub fn to_wire(command: &Command) -> Option<PanelCommand> {
    Some(match command {
        Command::SayToCarl(text) => PanelCommand::Say { text: text.clone() },
        Command::SetObjective(text) => PanelCommand::Objective { text: text.clone() },
        Command::AnswerDecision { id, answer } => PanelCommand::Answer {
            // The id is the sequence that asked, kept as a string on the way through so the
            // screen never has to hold a number it does not use.
            seq: id.parse().unwrap_or_default(),
            text: answer.clone(),
        },
        Command::Intervene(i) => match i.kind {
            InterventionKind::Message => PanelCommand::JjMessage {
                agent: i.agent.clone(),
                text: i.body.clone(),
            },
            InterventionKind::ChangeInstruction => PanelCommand::JjInstruct {
                agent: i.agent.clone(),
                instruction: i.body.clone(),
            },
            InterventionKind::StopTask => PanelCommand::JjStop {
                agent: i.agent.clone(),
                why: i.body.clone(),
            },
            InterventionKind::ReplaceTask => PanelCommand::JjReplace {
                agent: i.agent.clone(),
                goal: i.body.clone(),
                why: "replaced by JJ directly".into(),
            },
        },
        // The workspace is the panel's own container. Process 3 fills it, and nothing about
        // opening a pane belongs on the army's command channel.
        Command::Workspace(_) => return None,
    })
}

/// Puts fresh machine readings in place of the old ones.
///
/// Its own function, next to the event reducer and sharing nothing with it, because the two
/// must never drift into each other. An event says the army did something and carries a place
/// in a sequence. Telemetry says a number was measured again and carries no place at all.
///
/// A component the sample did not mention is left exactly as it was. A sampler that only reads
/// the machine must not silently delete the army rows beside them.
pub fn replace_telemetry(snapshot: &mut Snapshot, fresh: &[Diagnostic]) {
    for reading in fresh {
        match snapshot
            .diagnostics
            .iter_mut()
            .find(|d| d.component == reading.component)
        {
            Some(slot) => *slot = reading.clone(),
            None => snapshot.diagnostics.push(reading.clone()),
        }
    }
}

/// Folds one journal frame into what the screen holds.
pub fn from_event(wire: &WireEvent, snapshot: &mut Snapshot, out: &mut Vec<PanelEvent>) {
    // Always kept, because the record is the thing an agent's detail view lists and it is true
    // regardless of what else can be derived from it.
    out.push(PanelEvent::Recorded(Box::new(wire.record.clone())));

    match &wire.record.event {
        // The one change a record states outright.
        JournalEvent::Moved { task, to, .. } => {
            if let Some(held) = snapshot.tasks.iter_mut().find(|t| t.id == task.as_str()) {
                held.status = to.clone();
                held.updated_at = wire.at;
                out.push(PanelEvent::TaskChanged(Box::new(held.clone())));
            }
        }
        JournalEvent::Submitted { task, attempt, .. } => {
            if let Some(held) = snapshot.tasks.iter_mut().find(|t| t.id == task.as_str()) {
                held.attempts = *attempt;
                held.updated_at = wire.at;
                out.push(PanelEvent::TaskChanged(Box::new(held.clone())));
            }
        }
        JournalEvent::Delegated { task, to, goal, .. } => {
            out.push(PanelEvent::Delegated(Box::new(crate::model::Delegation {
                at: wire.at,
                from: wire.record.actor.clone(),
                to: to.clone(),
                goal: goal.clone(),
                task: Some(task.to_string()),
            })));
        }
        // Everything else is recorded and shown in the event list. Nothing further is derived,
        // because the record does not say what the world looks like afterwards and the next
        // snapshot does.
        _ => {}
    }

    // Whoever the frame is about has just done something, which is the one live overlay a
    // record does support: the name and the moment, taken from the frame rather than composed.
    let about = match &wire.entity {
        Entity::Agent { name } => Some(name.clone()),
        Entity::Task { actor, .. } => Some(actor.clone()),
    };
    if let Some(name) = about
        && let Some(agent) = snapshot.agents.iter_mut().find(|a| a.name == name)
    {
        agent.last_activity = Some(wire.kind.clone());
        agent.last_activity_at = Some(wire.at);
        out.push(PanelEvent::AgentChanged(Box::new(agent.clone())));
    }
}
