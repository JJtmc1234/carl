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
//! password prompt.
//!
//! A real pseudoterminal rather than a command runner. `portable-pty` was checked on this
//! machine before it was chosen: shell state persists across writes, `tty` reports a real
//! `/dev/pts` device, and resize works. A one command at a time runner cannot do any of that,
//! and a shell that cannot hold state is not a terminal.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::{Error, Result};

/// How much output is kept for redrawing. Beyond this the oldest is dropped.
///
/// Bounded because a runaway `yes` should cost a fixed amount of memory rather than all of it.
pub const SCROLLBACK_BYTES: usize = 256 * 1024;

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
    output: Receiver<Vec<u8>>,
    scrollback: Vec<u8>,
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

        let (tx, output) = channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(chunk[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let pid = child.process_id();
        Ok(Self {
            master: pair.master,
            child,
            writer,
            output,
            scrollback: Vec::new(),
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
    pub fn drain(&mut self) -> Vec<u8> {
        let mut fresh = Vec::new();
        // Both an empty channel and a hung up one mean there is nothing more to take right
        // now. A shell that has exited still leaves whatever it printed in the scrollback.
        while let Ok(chunk) = self.output.try_recv() {
            fresh.extend_from_slice(&chunk);
        }

        self.scrollback.extend_from_slice(&fresh);
        if self.scrollback.len() > SCROLLBACK_BYTES {
            let over = self.scrollback.len() - SCROLLBACK_BYTES;
            self.scrollback.drain(..over);
        }
        fresh
    }

    /// Everything still held for redrawing.
    pub fn scrollback(&self) -> &[u8] {
        &self.scrollback
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
