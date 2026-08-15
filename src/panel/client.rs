//! Talking to the panel backend, typed.
//!
//! One connection, one struct. Everything about sockets, line framing, request ids and JSON stops
//! here, so whatever draws the screen deals in snapshots and events and never in bytes.
//!
//! The protocol types are the backend's own. Nothing in this file redefines a frame, because a
//! client with its own copy of the schema is a client that can disagree with the server it is
//! talking to, and the compiler would not say a word.
//!
//! **Subscribing consumes the connection.** `subscribe` takes `self` and gives back an `Events`,
//! because once a panel is streaming there is no way to interleave a request on the same socket
//! without framing this protocol does not have. Making that a type change rather than a rule in a
//! document means it cannot be got wrong.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

use super::command::PanelCommand;
use super::view::PanelSnapshot;
use super::wire::{Ask, Frame, PanelEvent, Reply, Request, VERSION};
use crate::{Error, Result};

/// What came back from a command that changed something.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    /// Where it landed in the journal. `None` for a command that writes nothing.
    pub seq: Option<u64>,
    pub what: String,
}

/// One connection to the panel backend.
pub struct PanelClient {
    out: UnixStream,
    lines: BufReader<UnixStream>,
    next_id: u64,
}

impl PanelClient {
    /// Opens a connection. Fails if nothing is listening.
    pub fn connect(socket: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket).map_err(|e| {
            Error::Refused(format!(
                "no panel backend at {}: {e}. Start one with `carl panel`.",
                socket.display()
            ))
        })?;
        Ok(Self {
            out: stream.try_clone()?,
            lines: BufReader::new(stream),
            next_id: 0,
        })
    }

    /// How long to wait on a read before giving up.
    ///
    /// Used by the reconnect helper to notice a backend that has stopped answering without
    /// waiting forever. `None` restores blocking reads.
    pub fn read_timeout(&mut self, how_long: Option<Duration>) -> Result<()> {
        self.out.set_read_timeout(how_long)?;
        Ok(())
    }

    /// Is anyone there.
    pub fn ping(&mut self) -> Result<()> {
        match self.ask(Ask::Ping)? {
            Reply::Pong => Ok(()),
            other => Err(unexpected("pong", &other)),
        }
    }

    /// Everything, at one moment.
    pub fn snapshot(&mut self) -> Result<PanelSnapshot> {
        match self.ask(Ask::Snapshot)? {
            Reply::Snapshot { snapshot } => Ok(*snapshot),
            other => Err(unexpected("a snapshot", &other)),
        }
    }

    /// Does something, ignoring anything Carl says on the way.
    pub fn command(&mut self, command: PanelCommand) -> Result<Done> {
        self.command_streaming(command, &mut |_| {})
    }

    /// Does something, handing over Carl's answer as it arrives.
    ///
    /// `say` and `objective` produce `speaking` frames while Carl is still talking. Every other
    /// command produces none, and `on_text` is simply never called. Nothing here splits a
    /// finished answer into pieces to look like streaming: text arrives when the backend sends
    /// it or it does not arrive early at all.
    pub fn command_streaming(
        &mut self,
        command: PanelCommand,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Done> {
        let id = self.send(Ask::Command { command })?;
        loop {
            let frame = self.read()?;
            match frame.body {
                Reply::Speaking { text } => on_text(&text),
                Reply::Done { seq, what } => return Ok(Done { seq, what }),
                Reply::Refused { why } => return Err(Error::Refused(why)),
                other => return Err(unexpected_for(&id, "done", &other)),
            }
        }
    }

    /// Starts the live stream, giving up this connection to do it.
    ///
    /// `since` is the last sequence already held. Everything after it is replayed before the
    /// stream goes live, so a panel that was away sees what it missed.
    pub fn subscribe(mut self, since: u64) -> Result<Events> {
        self.send(Ask::Subscribe { since })?;
        Ok(Events {
            lines: self.lines,
            _out: self.out,
            last: since,
            caught_up: false,
        })
    }

    /// Sends one request and reads until its reply, skipping stream frames.
    fn ask(&mut self, body: Ask) -> Result<Reply> {
        let id = self.send(body)?;
        loop {
            let frame = self.read()?;
            // A frame with no id was not asked for. Nothing here subscribes, so this cannot
            // happen today, and dropping it silently would hide it if it ever did.
            match frame.id.as_deref() {
                Some(got) if got == id => {
                    return match frame.body {
                        Reply::Refused { why } => Err(Error::Refused(why)),
                        other => Ok(other),
                    };
                }
                _ => continue,
            }
        }
    }

    fn send(&mut self, body: Ask) -> Result<String> {
        self.next_id += 1;
        let id = format!("c{}", self.next_id);
        let request = Request {
            v: VERSION,
            id: id.clone(),
            body,
        };
        writeln!(self.out, "{}", serde_json::to_string(&request)?)?;
        self.out.flush()?;
        Ok(id)
    }

    fn read(&mut self) -> Result<Frame> {
        read_frame(&mut self.lines)
    }
}

/// The live stream, after subscribing.
pub struct Events {
    lines: BufReader<UnixStream>,
    /// Kept alive so the write half is not closed under the read half.
    _out: UnixStream,
    last: u64,
    caught_up: bool,
}

/// One thing off the live stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incoming {
    /// A journal record, in order.
    Event(Box<PanelEvent>),
    /// The replay is finished and everything after this is live.
    CaughtUp { seq: u64 },
    /// The backend cannot honour the sequence asked for. Take a fresh snapshot.
    Gap {
        asked_for: u64,
        have_from: u64,
        have_to: u64,
        why: String,
    },
}

impl Events {
    /// The highest sequence accepted so far. What to resume from after a reconnect.
    pub fn last_seq(&self) -> u64 {
        self.last
    }

    /// Whether the replay is finished.
    pub fn caught_up(&self) -> bool {
        self.caught_up
    }

    /// The next thing off the stream. Blocks until there is one.
    ///
    /// Named `recv` rather than `next` because it is not an iterator: the end of this stream is a
    /// failure to be handled and not a tidy `None`, and calling it `next` would invite a `for`
    /// loop that swallows exactly that.
    ///
    /// A sequence that is not exactly one past the last accepted one is an error rather than a
    /// value, because the whole promise of this stream is that it has no holes. Reporting it as
    /// a `Gap` would let a caller carry on with state that quietly skipped a record.
    pub fn recv(&mut self) -> Result<Incoming> {
        let frame = read_frame(&mut self.lines)?;
        match frame.body {
            Reply::Event { event } => {
                if event.seq != self.last + 1 {
                    return Err(Error::Refused(format!(
                        "the stream jumped from {} to {}. That is a hole, and this client will \
                         not carry on past one.",
                        self.last, event.seq
                    )));
                }
                self.last = event.seq;
                Ok(Incoming::Event(event))
            }
            Reply::Live { seq } => {
                self.caught_up = true;
                self.last = self.last.max(seq);
                Ok(Incoming::CaughtUp { seq })
            }
            Reply::Gap {
                asked_for,
                have_from,
                have_to,
                why,
            } => Ok(Incoming::Gap {
                asked_for,
                have_from,
                have_to,
                why,
            }),
            Reply::Refused { why } => Err(Error::Refused(why)),
            other => Err(unexpected("an event", &other)),
        }
    }
}

/// Reads one line and decodes it.
///
/// End of file is an error rather than `None`, because the backend closing mid stream is a thing
/// the caller has to handle and not a tidy end to a sequence.
fn read_frame(lines: &mut BufReader<UnixStream>) -> Result<Frame> {
    let mut line = String::new();
    match lines.read_line(&mut line)? {
        0 => Err(Error::Refused(
            "the panel backend closed the connection".into(),
        )),
        _ => serde_json::from_str(&line).map_err(|e| {
            Error::Refused(format!(
                "the panel backend sent something this client cannot read: {e}. Frame was {}",
                line.trim()
            ))
        }),
    }
}

fn unexpected(wanted: &str, got: &Reply) -> Error {
    Error::Refused(format!("expected {wanted} and the backend sent {got:?}"))
}

fn unexpected_for(id: &str, wanted: &str, got: &Reply) -> Error {
    Error::Refused(format!(
        "expected {wanted} for request {id} and the backend sent {got:?}"
    ))
}
