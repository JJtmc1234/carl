//! JJ's terminal. Not a tool, and not an agent's.
//!
//! This is the one piece of this work with a real security shape, so the boundary is worth
//! stating plainly rather than leaving implied.
//!
//! **It is JJ's.** It runs his login shell, as him, with his permissions, in a directory he
//! picked. It gains nothing he does not already have. There is no path in this module that
//! raises privilege, and the test `the_shell_runs_as_this_user_and_gains_nothing` proves the
//! user id inside the terminal is the user id outside it.
//!
//! **No agent gets one.** Nothing here is registered as a tool, named in an allow list, or
//! reachable from `claude::Runner`. Agent commands go through the sandbox in `etc/carl-python`
//! and the tool allow list, and they must keep going through it. If a future change ever wants
//! to give an agent a shell, that is a governance decision for JJ and not something this
//! module should make easy by accident.
//!
//! **Nothing is captured.** Output is held in a bounded ring in memory so the panel can draw
//! it, and it is never written to disk, never parsed for content, and never logged. A terminal
//! that scraped its own output would be a terminal that recorded whatever JJ typed into a
//! password prompt. The ring is bounded by the thread that fills it rather than by the caller
//! that empties it, so the sentence above holds for a caller that never drains at all.
//!
//! A real pseudoterminal rather than a command runner. `portable-pty` was checked on this
//! machine before it was chosen: shell state persists across writes, `tty` reports a real
//! `/dev/pts` device, and resize works. A one command at a time runner cannot do any of that,
//! and a shell that cannot hold state is not a terminal.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::{Error, Result};

/// How much output is kept for redrawing. Beyond this the oldest is dropped.
///
/// Bounded because a runaway `yes` should cost a fixed amount of memory rather than all of it.
///
/// This is the whole budget, not the budget after somebody drains. The reader thread used to
/// push every chunk into an unbounded channel and this cap only applied once `drain` had moved
/// the bytes out of it, so the real cost was whatever the shell printed between two drains. A
/// `yes` in a pane the panel was not drawing grew at about 40 MiB a second. See bug 13.
pub const SCROLLBACK_BYTES: usize = 256 * 1024;

/// The single buffer the reader thread writes into and `drain` reads out of.
///
/// One buffer rather than a queue in front of a buffer, because two places to put bytes means
/// only one of them can be the one that is bounded.
#[derive(Default)]
struct Ring {
    /// Never longer than [`SCROLLBACK_BYTES`].
    bytes: Vec<u8>,
    /// Where the output nobody has drained yet starts. Moves down when the front is trimmed.
    fresh_from: usize,
    /// How much was thrown away for being over the cap, so the loss can be seen rather than
    /// guessed at. A terminal that quietly eats output looks like a shell that said nothing.
    dropped: u64,
}

impl Ring {
    /// Appends, then trims the front so the cap holds at all times rather than at drain time.
    fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if self.bytes.len() > SCROLLBACK_BYTES {
            let over = self.bytes.len() - SCROLLBACK_BYTES;
            self.bytes.drain(..over);
            // Trimming the front moves the undrained mark with it. Without this the mark would
            // point past bytes that no longer exist, and the next drain would panic or return
            // the wrong window.
            self.fresh_from = self.fresh_from.saturating_sub(over);
            self.dropped += over as u64;
        }
    }
}

/// Environment that must not be inherited into the shell.
///
/// The same list `capture.rs` scrubs, for the same reason. Carl may be started from inside a
/// snap, and a leaked `LD_LIBRARY_PATH` makes ordinary system binaries die with an undefined
/// symbol error that looks like the terminal is broken rather than like the environment is.
const SCRUB: &[&str] = &[
    "LD_LIBRARY_PATH",
    "LD_PRELOAD",
    "SNAP",
    "SNAP_NAME",
    "SNAP_REVISION",
    "GTK_PATH",
    "GIO_MODULE_DIR",
    "GSETTINGS_SCHEMA_DIR",
];

/// Strips the inherited environment that must not reach the shell, and sets what must.
///
/// A function rather than a loop inside `open`, so a test can call the thing that actually
/// runs and check the result. The previous test asserted that `SCRUB` still listed the right
/// names, which is a different claim: delete the removal and that test still passes while
/// every variable leaks. A test that cannot fail for the real reason is worse than none,
/// because it is evidence pointing the wrong way.
fn scrub(cmd: &mut CommandBuilder) {
    for name in SCRUB {
        cmd.env_remove(name);
    }
    // Says to the shell and to anything it runs that this is an interactive terminal.
    cmd.env("TERM", "xterm-256color");
}

/// How big the terminal is, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub rows: u16,
    pub cols: u16,
}

impl Default for Size {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

impl From<Size> for PtySize {
    fn from(s: Size) -> Self {
        PtySize {
            rows: s.rows,
            cols: s.cols,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

/// One interactive shell.
pub struct Terminal {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    ring: Arc<Mutex<Ring>>,
    started_in: PathBuf,
    pid: Option<u32>,
}

impl Terminal {
    /// Opens a shell in `cwd`.
    ///
    /// The shell is `$SHELL`, falling back to `/bin/bash`, because this is JJ's terminal and it
    /// should behave the way his terminal behaves.
    pub fn open(cwd: impl AsRef<Path>, size: Size) -> Result<Self> {
        let cwd = cwd.as_ref();
        if !cwd.is_dir() {
            return Err(Error::Refused(format!(
                "{} is not a directory to start a terminal in",
                cwd.display()
            )));
        }

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let pair = native_pty_system()
            .openpty(size.into())
            .map_err(|e| Error::Refused(format!("could not open a pseudoterminal: {e}")))?;

        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(cwd);
        scrub(&mut cmd);

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| Error::Refused(format!("could not start {shell}: {e}")))?;
        // Dropped so the master sees end of file once the shell exits, rather than hanging on
        // a copy of the slave this process is still holding open.
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| Error::Refused(format!("could not read the terminal: {e}")))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| Error::Refused(format!("could not write to the terminal: {e}")))?;

        let ring = Arc::new(Mutex::new(Ring::default()));
        let writing = Arc::clone(&ring);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    // Trimmed here, in the thread that produces the bytes, so the cap does not
                    // depend on anybody calling `drain`. The reader is never blocked: it drops
                    // the oldest output instead, because stalling the pty read would push the
                    // backlog into the kernel and hang the shell rather than bound anything.
                    Ok(n) => match writing.lock() {
                        Ok(mut ring) => ring.push(&chunk[..n]),
                        // The only way to get here is a panic while the lock was held. There is
                        // nowhere left to put the bytes, so the thread stops rather than
                        // spinning on a lock that will never be good again.
                        Err(_) => break,
                    },
                }
            }
        });

        let pid = child.process_id();
        Ok(Self {
            master: pair.master,
            child,
            writer,
            ring,
            started_in: cwd.to_path_buf(),
            pid,
        })
    }

    /// Sends what JJ typed, exactly as typed.
    pub fn send(&mut self, input: &str) -> Result<()> {
        self.writer
            .write_all(input.as_bytes())
            .map_err(|e| Error::Refused(format!("the terminal would not take input: {e}")))?;
        self.writer.flush()?;
        Ok(())
    }

    /// Sends a line, adding the newline the shell is waiting for.
    pub fn send_line(&mut self, line: &str) -> Result<()> {
        self.send(&format!("{line}\n"))
    }

    /// Everything printed since the last call, and nothing that was read before it.
    ///
    /// Capped by [`SCROLLBACK_BYTES`] like the scrollback is, because it is now a window onto
    /// the same buffer rather than a queue that had been filling up in front of it. A caller
    /// that stops draining for a minute gets the last 256 KiB, not the whole minute.
    pub fn drain(&mut self) -> Vec<u8> {
        let mut ring = self.locked();
        let fresh = ring.bytes[ring.fresh_from..].to_vec();
        ring.fresh_from = ring.bytes.len();
        fresh
    }

    /// Everything still held for redrawing.
    ///
    /// A copy rather than a borrow, since the reader thread is appending to the same buffer and
    /// a reference out of the lock would be a reference to something being written.
    pub fn scrollback(&self) -> Vec<u8> {
        self.locked().bytes.clone()
    }

    /// How much output has been thrown away for being over the cap.
    ///
    /// Exposed so a caller can say output was lost rather than let it disappear. Nothing in
    /// this module decides what to do about it.
    pub fn dropped(&self) -> u64 {
        self.locked().dropped
    }

    /// The ring, taking the poison as an empty read rather than a panic.
    ///
    /// A poisoned lock means the reader thread died mid append. The terminal is still usable
    /// for input and the shell is still alive, so bringing the panel down over it would be a
    /// worse outcome than a redraw that is missing some output.
    fn locked(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Where the terminal was started.
    pub fn started_in(&self) -> &Path {
        &self.started_in
    }

    /// Where the shell is *now*, which is what the panel should show.
    ///
    /// Read from `/proc/<pid>/cwd` rather than tracked, so it stays right after JJ types `cd`.
    /// Asking the shell would mean writing to it and reading the answer back out of the same
    /// stream JJ is using, which would put text on his screen that he did not type.
    pub fn current_dir(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.pid?)).ok()
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn resize(&self, size: Size) -> Result<()> {
        self.master
            .resize(size.into())
            .map_err(|e| Error::Refused(format!("could not resize the terminal: {e}")))
    }

    /// Whether the shell is still there.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Ends the session and reaps the shell.
    ///
    /// Killing rather than asking politely, because this is called when JJ closes the pane and
    /// a shell sitting in a full screen editor will not act on an `exit` it never reads.
    pub fn close(&mut self) -> Result<()> {
        let _ = self.child.kill();
        self.child
            .wait()
            .map_err(|e| Error::Refused(format!("the shell would not be reaped: {e}")))?;
        Ok(())
    }
}

impl Drop for Terminal {
    /// A closed panel must not leave a shell running.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests;
