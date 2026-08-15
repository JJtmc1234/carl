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
}

impl LivePanelDataSource {
    /// Connects, takes the first authoritative snapshot, and starts listening.
    ///
    /// Fails rather than starting empty. A panel that opens against nothing and looks like an
    /// army with no agents is worse than one that says it could not connect.
    pub fn open(socket: &Path) -> Result<Self, String> {
        let (live, first) = LivePanel::open(socket).map_err(|e| e.to_string())?;

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
        let mut out = Vec::new();
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
                self.replace(*fresh);
                out.push(PanelEvent::Resynced(Box::new(self.latest.clone())));
            }
            Update::Health(health) => {
                self.link = link_of(health);
                out.push(PanelEvent::LinkChanged(self.link.clone()));
            }
            Update::Event(event) => translate::from_event(&event, &mut self.latest, out),
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
