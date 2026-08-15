//! Staying connected, and being honest when it is not.
//!
//! The reconnect loop from `docs/panel-v1.md`, written once here so no future panel has to write
//! it again and get one of the corners wrong.
//!
//! **A quiet army and a dead backend look identical from a subscribed socket**, because the
//! backend sends nothing when nothing is happening. Silence therefore proves nothing, and a
//! client that treated a long quiet as healthy would show a live looking screen after the backend
//! had gone. So when the stream goes quiet this opens a second short lived connection and pings.
//! An answer means the army is quiet. No answer means the backend is gone, and the screen is
//! entitled to say so.
//!
//! **Nothing here guesses.** A sequence that is not exactly the next one is refused by `Events`
//! rather than accepted, and a gap the backend reports is answered with a fresh snapshot rather
//! than by carrying on from a number that looked close enough.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::client::{Done, Events, Incoming, PanelClient};
use super::command::PanelCommand;
use super::view::PanelSnapshot;
use super::wire::PanelEvent;
use crate::Result;

/// How long a connection may be silent before the client goes and checks.
pub const QUIET: Duration = Duration::from_secs(5);

/// How long to wait between reconnection attempts.
pub const RETRY: Duration = Duration::from_millis(250);

/// What the screen is entitled to claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Contact confirmed recently. What is on screen is current.
    Connected,
    /// The connection has gone and the client is trying again.
    Reconnecting,
    /// Connected, but nothing has been heard for a while and a check has not yet confirmed the
    /// backend is alive. What is on screen was true and may not be now.
    Stale,
    /// Not connected, and attempts are failing. Nothing on screen is current.
    Disconnected,
}

/// Something worth telling the screen about.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// One journal record, in order, with no hole before it.
    Event(Box<PanelEvent>),
    /// The stream could not be resumed, so here is fresh truth instead.
    ///
    /// Throw away what you were holding and rebuild from this. It carries its own sequence, and
    /// the stream continues from exactly there.
    Resynced(Box<PanelSnapshot>),
    /// Fresh machine readings, replacing the ones in the last snapshot.
    ///
    /// Sampled telemetry only. Nothing about the army arrives this way, and this carries no
    /// sequence: replace the diagnostics you are holding and leave everything else alone.
    Telemetry {
        at: u64,
        diagnostics: Vec<crate::providers::health::Diagnostic>,
    },
    /// Nothing happened except that the honest description of the connection changed.
    Health(Health),
}

/// Whether a read failed because nothing arrived in time, rather than because the connection
/// broke.
///
/// A read timeout is the ordinary case of a quiet army and must not be mistaken for the backend
/// having gone. Linux reports it as `WouldBlock` and some platforms as `TimedOut`, so both count.
fn timed_out(e: &crate::Error) -> bool {
    match e {
        crate::Error::Io(io) => matches!(
            io.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
        _ => false,
    }
}

/// A panel connection that keeps itself up.
pub struct LivePanel {
    socket: PathBuf,
    events: Option<Events>,
    last: u64,
    health: Health,
    confirmed_at: Instant,
    quiet_after: Duration,
    /// Updates produced together and handed out one at a time.
    ///
    /// One turn of the loop can produce two things worth saying: the connection came back *and*
    /// here is the event that proved it. A queue keeps both rather than making the caller infer
    /// one from the other.
    pending: VecDeque<Update>,
}

impl LivePanel {
    /// Connects, takes a snapshot, and subscribes from exactly its sequence.
    ///
    /// The snapshot comes back so the caller can draw something before the first event. The
    /// sequence it carries is the join: the next frame on the stream is the next record.
    pub fn open(socket: &Path) -> Result<(Self, PanelSnapshot)> {
        let mut client = PanelClient::connect(socket)?;
        let snapshot = client.snapshot()?;

        let mut live = Self {
            socket: socket.to_path_buf(),
            events: None,
            last: snapshot.seq,
            health: Health::Connected,
            confirmed_at: Instant::now(),
            quiet_after: QUIET,
            pending: VecDeque::new(),
        };
        live.resubscribe()?;
        Ok((live, snapshot))
    }

    /// How long the stream may be silent before the client goes and checks. For tests.
    pub fn quiet_after(mut self, how_long: Duration) -> Self {
        self.quiet_after = how_long;
        self
    }

    pub fn health(&self) -> Health {
        self.health
    }

    /// The highest sequence accepted. Nothing before it was skipped.
    pub fn last_seq(&self) -> u64 {
        self.last
    }

    /// How long since contact with the backend was last confirmed.
    pub fn since_contact(&self) -> Duration {
        self.confirmed_at.elapsed()
    }

    /// Sends a command on a connection of its own.
    ///
    /// Its own, because this one is subscribed and a subscribed connection cannot carry a
    /// request. Short lived, because a command is rare and a second held socket is a second
    /// thing to keep alive.
    pub fn command(&mut self, command: PanelCommand) -> Result<Done> {
        self.command_streaming(command, &mut |_| {})
    }

    /// The same, with Carl's answer handed over as it arrives.
    pub fn command_streaming(
        &mut self,
        command: PanelCommand,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<Done> {
        let mut client = PanelClient::connect(&self.socket)?;
        let done = client.command_streaming(command, on_text)?;
        self.confirm();
        Ok(done)
    }

    /// The next thing worth telling the screen. Blocks until there is one.
    ///
    /// Also reachable as an `Iterator`, which never ends: a panel that cannot reach its backend
    /// says `Disconnected` and keeps trying, because a screen going blank is not the answer.
    ///
    /// Run it on a thread and forward what it gives you. It only returns when something actually
    /// happened, so a caller does not have to filter out ticks.
    pub fn next_update(&mut self) -> Update {
        loop {
            if let Some(ready) = self.pending.pop_front() {
                return ready;
            }
            if self.events.is_none() {
                match self.recover() {
                    Some(update) => return update,
                    None => continue,
                }
            }

            let got = self.events.as_mut().expect("just checked").recv();
            match got {
                Ok(Incoming::Event(event)) => {
                    self.last = event.seq;
                    self.confirm();
                    // Health first when it changed, then the event that proved it. Both are
                    // queued so neither is dropped in favour of the other.
                    if let Some(update) = self.became(Health::Connected) {
                        self.pending.push_back(update);
                    }
                    self.pending.push_back(Update::Event(event));
                }
                Ok(Incoming::Telemetry { at, diagnostics }) => {
                    // Contact, so the connection is alive even when the army is quiet. It does
                    // not move `last`, because the resume point belongs to the journal.
                    self.confirm();
                    if let Some(update) = self.became(Health::Connected) {
                        self.pending.push_back(update);
                    }
                    self.pending
                        .push_back(Update::Telemetry { at, diagnostics });
                }
                Ok(Incoming::CaughtUp { seq }) => {
                    self.last = self.last.max(seq);
                    self.confirm();
                    if let Some(update) = self.became(Health::Connected) {
                        return update;
                    }
                }
                // The backend cannot honour where we were. Only a fresh snapshot is honest.
                Ok(Incoming::Gap { .. }) => {
                    self.events = None;
                    if let Some(update) = self.resync() {
                        return update;
                    }
                }
                Err(e) if timed_out(&e) => {
                    if let Some(update) = self.check_still_there() {
                        return update;
                    }
                }
                // The connection went. Anything on screen is now history.
                Err(_) => {
                    self.events = None;
                    if let Some(update) = self.became(Health::Reconnecting) {
                        return update;
                    }
                }
            }
        }
    }

    fn confirm(&mut self) {
        self.confirmed_at = Instant::now();
    }

    /// Records a health change, and says so only when it actually changed.
    fn became(&mut self, health: Health) -> Option<Update> {
        if self.health == health {
            return None;
        }
        self.health = health;
        Some(Update::Health(health))
    }

    /// The stream has been quiet. Find out whether that is peace or death.
    fn check_still_there(&mut self) -> Option<Update> {
        if self.since_contact() < self.quiet_after {
            return None;
        }
        match PanelClient::connect(&self.socket).and_then(|mut c| c.ping()) {
            // Alive and the army is simply quiet, which is the ordinary case.
            Ok(()) => {
                self.confirm();
                self.became(Health::Connected)
            }
            Err(_) => {
                self.events = None;
                self.became(Health::Stale)
            }
        }
    }

    /// Reconnect, resume if the backend can, and resnapshot if it cannot.
    fn recover(&mut self) -> Option<Update> {
        std::thread::sleep(RETRY);
        match self.resubscribe() {
            Ok(()) => self.became(Health::Connected),
            Err(_) => self.became(Health::Disconnected),
        }
    }

    /// Opens the stream from exactly where this client got to.
    fn resubscribe(&mut self) -> Result<()> {
        let mut client = PanelClient::connect(&self.socket)?;
        client.read_timeout(Some(self.quiet_after))?;
        self.events = Some(client.subscribe(self.last)?);
        self.confirm();
        Ok(())
    }

    /// Throws away where we were and takes fresh truth.
    fn resync(&mut self) -> Option<Update> {
        let mut client = match PanelClient::connect(&self.socket) {
            Ok(c) => c,
            Err(_) => return self.became(Health::Disconnected),
        };
        let snapshot = match client.snapshot() {
            Ok(s) => s,
            Err(_) => return self.became(Health::Disconnected),
        };

        self.last = snapshot.seq;
        if self.resubscribe().is_err() {
            return self.became(Health::Disconnected);
        }
        self.health = Health::Connected;
        Some(Update::Resynced(Box::new(snapshot)))
    }
}

impl Iterator for LivePanel {
    type Item = Update;

    /// Never `None`. A live panel that has lost its backend reports that and carries on trying,
    /// so there is no state in which this stream is finished.
    fn next(&mut self) -> Option<Update> {
        Some(self.next_update())
    }
}
