//! The real source, wired to Process 1's typed client.
//!
//! Everything about the transport stops here. Above this file nothing knows there is a socket,
//! a sequence number, a gap or a frame. `LivePanel` already hides the wire, and this hides
//! `LivePanel` behind the same four methods the mock implements, so the interface cannot tell
//! which one it is attached to.
//!
//! Two threads, because the client blocks and a UI must not.
//!
//! **The reader** owns the `LivePanel` and sits in `next_update()`, which blocks until
//! something has actually happened. It forwards what it gets down a channel. Blocking is
//! exactly what that thread is for.
//!
//! **The commander** takes one command at a time and runs it. It uses its own short lived
//! `PanelClient` rather than the reader's `LivePanel`, for the reason Process 1 documented on
//! `LivePanel::command`: a subscribed connection cannot carry a request. The reader is parked
//! inside a blocking call and cannot be borrowed to send one, so the command opens its own,
//! which is what `LivePanel::command_streaming` does internally anyway.
//!
//! Nothing is applied optimistically. A command goes out and the screen changes when an event
//! comes back saying it happened, which is the same rule the mock follows and the reason the
//! two are interchangeable.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use carl::panel::client::PanelClient;
use carl::panel::live::{Health, LivePanel, Update};
use carl::panel::view::PanelSnapshot;

use super::{PanelDataSource, PanelEvent, mapping};
use crate::command::Command;
use crate::model::{Link, Snapshot};

mod translate;

/// What the reader and commander threads send back.
enum FromBackend {
    Update(Box<Update>),
    /// A piece of Carl's answer, as it arrives.
    Speaking(String),
    /// A command finished, with whether it was accepted.
    Settled(Result<(), String>),
}

pub struct LivePanelDataSource {
    socket: PathBuf,
    /// The authoritative snapshot, replaced whole on every resync.
    latest: Snapshot,
    incoming: Receiver<FromBackend>,
    orders: Sender<Command>,
    link: Link,
    /// True while Carl is mid answer, so the end of the stream can close the turn.
    speaking: bool,
    /// Said on this side and not by the backend: JJ's own words, and the caret that shows Carl
    /// was asked. Queued rather than sent straight to the screen because `submit` is not the
    /// place that draws.
    echoed: Vec<PanelEvent>,
    /// The journal sequence the screen is caught up to.
    ///
    /// Advanced by army events and by a resync, and by nothing else. Telemetry carries no
    /// sequence and must never move this, because this is what a reconnection resumes from: a
    /// number pushed past the journal would ask the backend to continue from a record that
    /// never existed, and everything between would be silently skipped.
    last_seq: u64,
}

impl LivePanelDataSource {
    /// Connects, takes the first authoritative snapshot, and starts listening.
    ///
    /// Fails rather than starting empty. A panel that opens against nothing and looks like an
    /// army with no agents is worse than one that says it could not connect.
    pub fn open(socket: &Path) -> Result<Self, String> {
        let (live, first) = LivePanel::open(socket).map_err(|e| e.to_string())?;
        let first_seq = first.seq;

        let (tx, incoming) = channel();
        let (orders, take_orders) = channel::<Command>();

        reader(live, tx.clone());
        commander(socket.to_path_buf(), take_orders, tx);

        Ok(Self {
            socket: socket.to_path_buf(),
            latest: mapping::snapshot(first),
            incoming,
            orders,
            link: Link::Live,
            speaking: false,
            echoed: Vec::new(),
            last_seq: first_seq,
        })
    }

    /// The same source with the threads replaced by channels the test holds.
    ///
    /// Exists so the drain, the resync and the refusal can be checked without a backend. The
    /// alternative is testing through a real socket, where every failure arrives as a timeout
    /// instead of as an assertion.
    #[cfg(test)]
    fn detached(first: Snapshot) -> (Self, Sender<FromBackend>, Receiver<Command>) {
        let (tx, incoming) = channel();
        let (orders, taken) = channel();
        (
            Self {
                socket: PathBuf::from("/nonexistent.sock"),
                latest: first,
                incoming,
                orders,
                link: Link::Live,
                speaking: false,
                echoed: Vec::new(),
                last_seq: 0,
            },
            tx,
            taken,
        )
    }

    /// Where the backend puts its socket, so the usual case needs no argument.
    pub fn default_socket() -> PathBuf {
        carl::panel::listen::socket_path(&home())
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The journal sequence the screen is caught up to.
    ///
    /// Exposed so the separation between the two kinds of update can be asserted rather than
    /// reasoned about. Telemetry must never move it.
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }
}

/// The blocking reader.
fn reader(mut live: LivePanel, tx: Sender<FromBackend>) {
    thread::spawn(move || {
        loop {
            let update = live.next_update();
            if tx.send(FromBackend::Update(Box::new(update))).is_err() {
                // The panel has gone. Nothing to report to.
                break;
            }
        }
    });
}

/// The command runner, one at a time and never on the UI thread.
fn commander(socket: PathBuf, orders: Receiver<Command>, tx: Sender<FromBackend>) {
    thread::spawn(move || {
        for order in orders {
            // Answered first, because it is not a `PanelCommand` and never becomes one. A
            // process is holding a tool call still for this, so it goes on its own connection
            // and does not queue behind whatever Carl is in the middle of saying.
            if let Command::AnswerPermission { question, allow } = &order {
                let verdict = match allow {
                    true => carl::panel::permission::Verdict::Allow,
                    false => carl::panel::permission::Verdict::Deny,
                };
                // The backend's own words, not a blanket "sent". Answering a question that
                // has already expired is not a failure to send, it is an answer that landed on
                // nothing, and the two looked identical: JJ pressed Allow on a question that
                // had timed out and the panel told him it was sent.
                let outcome = PanelClient::connect(&socket)
                    .and_then(|mut client| client.answer(question, verdict))
                    .map_err(|e| e.to_string())
                    .and_then(|done| {
                        if done.what.contains("nothing was waiting") {
                            Err("too late, that one had already timed out and been refused"
                                .to_string())
                        } else {
                            Ok(())
                        }
                    });
                if tx.send(FromBackend::Settled(outcome)).is_err() {
                    break;
                }
                continue;
            }

            let Some(wire) = translate::to_wire(&order) else {
                // Workspace requests never reach the backend in this build. Process 3 owns
                // what fills the pane, and the panel opens the container itself.
                let _ = tx.send(FromBackend::Settled(Ok(())));
                continue;
            };

            let outcome = PanelClient::connect(&socket)
                .and_then(|mut client| {
                    let mut sink = |text: &str| {
                        let _ = tx.send(FromBackend::Speaking(text.to_string()));
                    };
                    client.command_streaming(wire, &mut sink)
                })
                .map(|_done| ())
                .map_err(|e| e.to_string());

            if tx.send(FromBackend::Settled(outcome)).is_err() {
                break;
            }
        }
    });
}

impl PanelDataSource for LivePanelDataSource {
    fn snapshot(&mut self) -> Snapshot {
        self.latest.clone()
    }

    fn poll(&mut self) -> Vec<PanelEvent> {
        // Anything said locally goes out first, so JJ's own line is above the answer to it.
        let mut out: Vec<PanelEvent> = self.echoed.drain(..).collect();
        loop {
            match self.incoming.try_recv() {
                Ok(FromBackend::Update(update)) => self.take(*update, &mut out),
                Ok(FromBackend::Speaking(text)) => {
                    self.speaking = true;
                    out.push(PanelEvent::CarlSaid {
                        text,
                        streaming: true,
                    });
                }
                Ok(FromBackend::Settled(result)) => {
                    // The end of an answer closes the turn, so the caret goes out only when the
                    // backend actually stopped talking.
                    if self.speaking {
                        self.speaking = false;
                        out.push(PanelEvent::CarlSaid {
                            text: String::new(),
                            streaming: false,
                        });
                    }
                    if let Err(why) = result {
                        out.push(PanelEvent::CommandRefused(why));
                    }
                }
                Err(TryRecvError::Empty) => break,
                // Both threads have gone, which means the process is going down with them.
                Err(TryRecvError::Disconnected) => {
                    self.link = Link::Disconnected {
                        why: "the panel client stopped".into(),
                    };
                    out.push(PanelEvent::LinkChanged(self.link.clone()));
                    break;
                }
            }
        }
        out
    }

    /// Sends, and refuses out loud rather than queueing.
    ///
    /// A command held back until the link returns is one JJ believes was sent. By the time it
    /// arrives the reason for it may be gone, and stopping a task that already finished is a
    /// worse outcome than being told it did not go.
    fn submit(&mut self, command: Command) -> Result<(), String> {
        if !self.link.is_live() {
            return Err(format!("not sent, {}", self.link.label().to_lowercase()));
        }

        // What JJ typed goes on screen here, not when the backend gets round to mentioning it,
        // because the backend never does: it answers, it does not echo. Without this the words
        // leave the box and appear nowhere, which reads as the panel having dropped them.
        //
        // The empty streaming turn is the thinking state. Carl takes seconds to answer and the
        // gap between sending and his first word was silent, so there was nothing to tell a
        // person their message had been taken. It is replaced by his real first words.
        match &command {
            Command::SayToCarl(text) => {
                self.echoed.push(PanelEvent::JjSaid(text.clone()));
                self.echoed.push(PanelEvent::CarlSaid {
                    text: String::new(),
                    streaming: true,
                });
                self.speaking = true;
            }
            Command::SetObjective(goal) => {
                self.echoed
                    .push(PanelEvent::JjSaid(format!("New objective. {goal}")));
                self.echoed.push(PanelEvent::CarlSaid {
                    text: String::new(),
                    streaming: true,
                });
                self.speaking = true;
            }
            _ => {}
        }

        self.orders
            .send(command)
            .map_err(|_| "the panel client has stopped".to_string())
    }

    fn link(&self) -> Link {
        self.link.clone()
    }

    fn describe(&self) -> String {
        format!("live, {}", self.socket.display())
    }
}

impl LivePanelDataSource {
    /// Folds one backend update into what the screen holds.
    fn take(&mut self, update: Update, out: &mut Vec<PanelEvent>) {
        match update {
            // Fresh truth. The old state is thrown away rather than merged, because whatever
            // happened during the gap was never delivered and laying the next event on top of
            // it would leave a version nobody ever sent.
            Update::Resynced(fresh) => {
                // A resync carries its own sequence and the stream continues from exactly
                // there, so this is the one thing other than an event that may move it.
                self.last_seq = fresh.seq;
                self.replace(*fresh);
                out.push(PanelEvent::Resynced(Box::new(self.latest.clone())));
            }
            Update::Health(health) => {
                self.link = link_of(health);
                out.push(PanelEvent::LinkChanged(self.link.clone()));
            }
            // Machine readings replace the machine readings and touch nothing else. No
            // sequence is invented for it, because it has none and the stream's position is
            // the journal's business.
            Update::Telemetry { at, diagnostics } => {
                translate::replace_telemetry(&mut self.latest, &diagnostics);
                out.push(PanelEvent::TelemetryChanged { at, diagnostics });
            }
            // Neither moves the sequence, for the same reason telemetry does not. A question is
            // not a journal record.
            Update::Asked(request) => {
                out.push(PanelEvent::PermissionAsked(Box::new(
                    crate::model::Permission {
                        id: request.id,
                        tool: request.tool,
                        detail: request.detail,
                        surface: request.surface,
                        asked_at: request.at,
                    },
                )));
            }
            Update::Answered { question, verdict } => {
                out.push(PanelEvent::PermissionSettled {
                    id: question,
                    allowed: verdict == carl::panel::permission::Verdict::Allow,
                });
            }
            Update::Event(event) => {
                // The only thing that advances the sequence. Taken from the frame rather than
                // counted here, so a replay after a reconnection lands on the same number the
                // backend has.
                self.last_seq = self.last_seq.max(event.seq);
                translate::from_event(&event, &mut self.latest, out);
            }
        }
    }

    fn replace(&mut self, fresh: PanelSnapshot) {
        // The conversation is the one thing a resync must not blank. It is this session's
        // talking, the backend keeps none of it, and throwing it away would punish JJ for a
        // dropped socket by wiping what he just said.
        let talking = std::mem::take(&mut self.latest.conversation);
        self.latest = mapping::snapshot(fresh);
        self.latest.conversation = talking;
        self.link = Link::Live;
    }
}

/// Their honest description of the connection, in the words the screen already uses.
pub fn link_of(health: Health) -> Link {
    match health {
        Health::Connected => Link::Live,
        Health::Reconnecting => Link::Connecting { attempt: 1 },
        Health::Stale => Link::Connecting { attempt: 0 },
        Health::Disconnected => Link::Disconnected {
            why: "the backend cannot be reached".into(),
        },
    }
}

fn home() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(h) => PathBuf::from(h).join(".carl"),
        None => PathBuf::from(".carl"),
    }
}

#[cfg(test)]
mod tests;
