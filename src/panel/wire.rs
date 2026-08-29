//! What the panel and the backend say to each other.
//!
//! One JSON object per line, in both directions, which is the same framing `aosd` uses and the
//! same framing the journal uses. It can be read with `nc` when the thing that is broken is the
//! backend itself, and that is worth more than a compact encoding at this size.
//!
//! **Ordering.** Every live update carries the journal sequence it came from. The journal is
//! the only ordering authority, so there is nothing here to keep in step with it. A panel that
//! has seen sequence 40 asks to continue from 40 and gets 41 onward. Nothing is renumbered and
//! nothing is invented, so a gap is a real gap rather than a disagreement between two counters.
//!
//! **Versioning.** Every frame carries `v`. A frame whose version the other side does not know
//! is refused with a message naming both versions, rather than being parsed optimistically and
//! misunderstood.

use serde::{Deserialize, Serialize};

use super::command::PanelCommand;
use super::view::PanelSnapshot;
use crate::army::event::Record;

/// The protocol version. Bumped when a frame changes shape in a way an older panel would
/// misread, and not for adding a field an older panel ignores.
pub const VERSION: u32 = 1;

/// Panel to backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub v: u32,
    /// The panel's own id for this request, echoed on the reply so a panel with several in
    /// flight can tell them apart.
    pub id: String,
    #[serde(flatten)]
    pub body: Ask,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "ask", rename_all = "snake_case")]
pub enum Ask {
    /// Is anyone there. Cheap, and the first thing a reconnecting panel sends.
    Ping,
    /// Everything, at one moment, with the sequence it was built from.
    Snapshot,
    /// Start the live stream.
    ///
    /// `since` is the last sequence the panel already has. Everything after it is replayed
    /// before the connection goes live, so a panel that was away sees what it missed rather
    /// than resuming from now and silently losing the middle.
    ///
    /// `since: 0` means from the beginning of the record.
    Subscribe { since: u64 },
    /// Do something. The half of the protocol that changes the world.
    Command { command: PanelCommand },
    /// A `PreToolUse` hook asking whether Carl may do something, and waiting for the answer.
    ///
    /// Takes over the connection, like `Subscribe` does, because the hook has nothing else to
    /// say and is holding a tool call still until it hears back.
    MayI {
        request: crate::panel::permission::Request,
    },
    /// JJ's answer to one of those.
    ///
    /// `question` rather than `id` because a frame already has an `id`, and the two are
    /// different things: the frame's correlates a reply with the request that caused it, and
    /// this one names the thing being decided. Both flattened into one object, so sharing the
    /// name is not a style question, it is a collision.
    Answered {
        question: String,
        verdict: crate::panel::permission::Verdict,
    },
}

/// Backend to panel.
///
/// Replies carry the `id` they answer. Stream frames have no `id`, because nothing asked for
/// them, and a panel must not treat one as a late answer to a request it made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(flatten)]
    pub body: Reply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reply", rename_all = "snake_case")]
pub enum Reply {
    Pong,
    Snapshot {
        snapshot: Box<PanelSnapshot>,
    },
    /// One journal record, as an ordered live update.
    ///
    /// The record is passed through rather than described in prose. A screen needs to count
    /// rejections and group by task, and a sentence cannot be counted.
    Event {
        event: Box<PanelEvent>,
    },
    /// The replay is finished and everything after this is live.
    ///
    /// Sent even when the replay was empty, so a panel always knows when it has caught up and
    /// never has to guess from a quiet connection.
    Live {
        seq: u64,
    },
    /// The panel asked to continue from a sequence the record cannot honour.
    ///
    /// Either it asked for a sequence past the end, which means it is talking to a different
    /// record than the one it remembers, or the beginning it needed has been lost. Either way
    /// the honest answer is to say so and let it take a fresh snapshot, rather than sending a
    /// stream with a hole in it that looks continuous.
    Gap {
        asked_for: u64,
        have_from: u64,
        have_to: u64,
        why: String,
    },
    /// A command was carried out.
    Done {
        /// The journal sequence it was recorded at, when it wrote one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        seq: Option<u64>,
        what: String,
    },
    /// Part of an answer Carl is still producing.
    ///
    /// Carl's turn machinery already hands text over as it arrives, so the panel gets the same
    /// stream the terminal and the microphone get rather than waiting for a finished paragraph.
    Speaking {
        text: String,
    },
    /// Part of Carl's reasoning, as he produces it.
    ///
    /// A frame of its own rather than more `Speaking`, because it is not the answer and a panel
    /// has to be able to put it somewhere else. Merged into the reply it would read as Carl
    /// talking to himself in the middle of a sentence.
    Thinking {
        text: String,
    },
    /// A tool Carl has just picked up.
    ///
    /// Split into the tool and the one detail worth reading rather than a pre-formatted line,
    /// so a panel can lay it out as it likes and a reader can count tool calls. A sentence
    /// cannot be counted, which is the same reason `PanelEvent` is a closed list.
    Doing {
        tool: String,
        detail: String,
    },
    /// Fresh machine readings, pushed because nothing else would have said.
    ///
    /// **This frame carries no sequence, deliberately.** Army events are ordered by the journal
    /// and that is the only ordering there is. A CPU number is not a thing that happened, it is
    /// a thing that was true when somebody looked, and giving it a sequence would put it in a
    /// line it does not belong in and let a panel believe telemetry and history are one stream.
    ///
    /// Sent only when the sampler actually took a new reading. Process 3's interval is the
    /// refresh rate, so there is no second one here to disagree with it, and a panel that
    /// receives one of these knows the numbers in it are genuinely newer than the last.
    Telemetry {
        /// When the sample was taken, which is not when this was sent.
        at: u64,
        diagnostics: Vec<crate::providers::health::Diagnostic>,
    },
    /// Carl is asking to do something and a person has to say. Pushed to every subscriber.
    ///
    /// Carries no sequence, for the same reason telemetry does not: it is a question being
    /// asked now rather than a place in the record of what happened. What Carl then did, or
    /// was stopped from doing, is the army's business and reaches the journal on its own.
    Permission {
        request: crate::panel::permission::Request,
    },
    /// The answer, so a panel that did not answer stops showing the question.
    Settled {
        question: String,
        verdict: crate::panel::permission::Verdict,
    },
    /// The request was refused, and by what rule.
    ///
    /// Refusals are values here for the same reason they are events in the journal: a rule
    /// nobody can see firing is a rule nobody can tell is working.
    Refused {
        why: String,
    },
}

/// One live update.
///
/// A thin wrapper around a journal record rather than a translation of it. `kind` and `entity`
/// are lifted out so a panel can route a frame without matching every payload shape, and
/// `record` is the untouched original so nothing is lost on the way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelEvent {
    pub seq: u64,
    pub at: u64,
    /// `delegated`, `moved`, `submitted`, `reviewed`, `refused`, `emergency_declared`,
    /// `decided`, `intervened`, `notified`.
    pub kind: String,
    /// What this is about, so a panel can filter without understanding the payload.
    pub entity: Entity,
    pub record: Record,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entity", rename_all = "snake_case")]
pub enum Entity {
    Task { id: String, actor: String },
    Agent { name: String },
}

impl PanelEvent {
    /// Lifts a journal record into a live update without changing it.
    ///
    /// The entity is who the frame is *about*, which is not always who acted. A notification is
    /// about the person notified, and an intervention about the agent it reached. Using the
    /// actor for those would file every JJ intervention under JJ, and the Agents tab would show
    /// nothing against the agent it actually happened to.
    pub fn of(record: Record) -> Self {
        let entity = match (record.event.task(), about(&record.event)) {
            (Some(id), _) => Entity::Task {
                id: id.to_string(),
                actor: record.actor.clone(),
            },
            (None, Some(name)) => Entity::Agent { name },
            (None, None) => Entity::Agent {
                name: record.actor.clone(),
            },
        };
        Self {
            seq: record.seq,
            at: record.at,
            kind: record.event.kind().to_string(),
            entity,
            record,
        }
    }
}

/// The agent a record concerns, when that is somebody other than whoever acted.
fn about(event: &crate::army::event::Event) -> Option<String> {
    use crate::army::event::Event;
    match event {
        Event::Notified { who, .. } => Some(who.clone()),
        Event::Intervened { what } => what.agent().map(str::to_string),
        _ => None,
    }
}

impl Frame {
    pub fn to(id: Option<String>, body: Reply) -> Self {
        Self {
            v: VERSION,
            id,
            body,
        }
    }

    pub fn refused(id: Option<String>, why: impl Into<String>) -> Self {
        Self::to(id, Reply::Refused { why: why.into() })
    }
}
