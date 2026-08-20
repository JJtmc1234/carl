//! One `claude` process, held open across many turns.
//!
//! Every turn used to start a new one. Measured, that costs about 0.8 seconds before a single
//! token is asked for, spent reading config and connecting servers, and then the model starts
//! from cold. A process kept open reaches its first token in 0.97 seconds where a fresh one
//! takes 2.8, and in a spoken conversation that is most of the wait.
//!
//! `--input-format stream-json` is what makes it possible. One JSON object per line on stdin,
//! each a user message, and the answers stream back on stdout in the same shape the one shot
//! mode uses.
//!
//! There is one real cost, and it decides the design. The system prompt is fixed when the
//! process starts, so anything that changes between turns cannot live there. Carl's memory
//! grows, his picture of the game is rewritten, and the game itself starts and stops. All of
//! that moves into the message instead. Only what never changes, which is who Carl is, stays
//! in the system prompt.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::thread::JoinHandle;

use serde_json::json;

/// How often to look up from waiting.
///
/// Short enough that giving up feels immediate, long enough that it is not a spin.
const TICK: std::time::Duration = std::time::Duration::from_millis(100);

/// How long to spend clearing an abandoned answer before giving up on it.
///
/// A session that will never produce its ending must not hang the next question forever. One
/// stale fragment is survivable. Carl never speaking again is not.
const DRAIN_LIMIT: std::time::Duration = std::time::Duration::from_secs(20);

use super::{Answer, Chunk, Flow, Runner, chunk_of};
use crate::{Error, Result, SessionId};

pub struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    chunks: Receiver<Chunk>,
    reader: Option<JoinHandle<()>>,
    /// True when a turn was abandoned before its answer finished.
    ///
    /// The model does not stop because the listener walked away. The rest of that answer is
    /// still coming down the pipe, and without this the next question would read it and hand
    /// back the end of the previous one.
    unfinished: bool,
}

impl Runner {
    /// The arguments a held open conversation is started with.
    ///
    /// Separate from starting it so the shape can be checked without a real `claude`, which
    /// matters because one wrong flag here is the difference between continuing a
    /// conversation and quietly starting a second one.
    pub fn session_args(&self, session: &SessionId, system: &str, resume: bool) -> Vec<String> {
        let mut args: Vec<String> = [
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--include-partial-messages",
            "--verbose",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // Pinning a new conversation and picking up an existing one are different flags, and
        // sending both is an error.
        args.push(if resume { "--resume" } else { "--session-id" }.into());
        args.push(session.to_string());

        if !system.is_empty() {
            args.push("--append-system-prompt".into());
            args.push(system.to_string());
        }
        args.extend(self.allowed_args());
        args
    }

    /// Starts a conversation and keeps it open.
    ///
    /// `system` is fixed for the life of the process. Anything that can change between turns
    /// belongs in the message, not here.
    pub fn open_session(
        &self,
        session: &SessionId,
        workdir: &Path,
        system: &str,
        resume: bool,
    ) -> Result<Session> {
        std::fs::create_dir_all(workdir)?;

        let mut child = Command::new(self.program())
            .args(self.session_args(session, system, resume))
            .current_dir(workdir)
            // Its own process group, so giving up on a session can end everything it started
            // and not just the process this handle points at. Claude spawns its own children,
            // MCP servers and shell tools, and they inherit this stdout pipe. Killing only
            // the direct child leaves them holding the write end, so the pipe never reaches
            // EOF and the reader thread below blocks for as long as a grandchild survives.
            // Found by testing the kill rather than assuming it. See bug 13.
            .process_group(0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Claude(format!("cannot run {}: {e}", self.program().display())))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Claude("no stdin on the session".into()))?;
        let out = child
            .stdout
            .take()
            .ok_or_else(|| Error::Claude("no stdout on the session".into()))?;

        // A reader thread, for the same reason the microphone has one. Speaking a sentence
        // blocks for as long as the sentence takes, and nothing may stop draining the child's
        // output while that happens.
        let (tx, chunks) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(out)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Some(chunk) = chunk_of(&line)
                    && tx.send(chunk).is_err()
                {
                    break;
                }
            }
        });

        Ok(Session {
            child,
            stdin: Some(stdin),
            chunks,
            reader: Some(reader),
            unfinished: false,
        })
    }
}

impl Session {
    /// Asks one question and reads the answer.
    ///
    /// `on_text` sees the words as they arrive. `while_waiting` is called every tenth of a
    /// second whether anything has arrived or not, which is what makes it possible to give up
    /// on an answer that has not started yet. Without it, changing your mind halfway through
    /// asking meant listening to the answer to the question you abandoned.
    pub fn ask(
        &mut self,
        prompt: &str,
        on_text: &mut dyn FnMut(&str) -> Flow,
        while_waiting: &mut dyn FnMut() -> Flow,
    ) -> Result<Answer> {
        if prompt.trim().is_empty() {
            return Err(Error::Claude(
                "refusing to ask claude an empty question".into(),
            ));
        }

        // Anything left over from a turn somebody walked away from, cleared before asking, so
        // this answer cannot be the tail of the last one.
        self.drain_unfinished();

        let message = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": prompt }] }
        });

        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| Error::Claude("this session has been closed".into()))?;
        writeln!(stdin, "{message}").map_err(|e| {
            Error::Claude(format!(
                "the session has gone away, so it needs reopening: {e}"
            ))
        })?;
        stdin.flush().ok();

        let mut said = String::new();

        loop {
            // A short wait rather than a blocking one, so giving up is possible before a
            // single word has arrived. That is the whole difference between interrupting Carl
            // while he talks, which already worked, and interrupting him while he thinks.
            match self.chunks.recv_timeout(TICK) {
                Ok(Chunk::Text(t)) => {
                    said.push_str(&t);
                    if on_text(&t) == Flow::Stop {
                        return Ok(self.abandon(said));
                    }
                }
                Ok(Chunk::Final(answer)) => return Ok(*answer),
                Err(RecvTimeoutError::Timeout) => {
                    if while_waiting() == Flow::Stop {
                        return Ok(self.abandon(said));
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        Err(Error::Claude(
            "the session ended without answering, so it needs reopening".into(),
        ))
    }

    /// Walks away from an answer without killing the conversation.
    ///
    /// The model carries on writing, which is fine, and the rest arrives with nobody reading
    /// it. Marked so the next question clears it out first rather than being handed the tail
    /// of this one.
    fn abandon(&mut self, said: String) -> Answer {
        self.unfinished = true;
        Answer {
            text: said,
            interrupted: true,
            session_id: None,
            cost_usd: None,
        }
    }

    /// Throws away the remains of an answer nobody waited for.
    ///
    /// Bounded, because a session that will never produce its ending must not hang the next
    /// question forever. Giving up on the drain is survivable: the worst case is one stale
    /// fragment, and the alternative is Carl never speaking again.
    fn drain_unfinished(&mut self) {
        if !self.unfinished {
            return;
        }
        let until = std::time::Instant::now() + DRAIN_LIMIT;
        while std::time::Instant::now() < until {
            match self.chunks.recv_timeout(TICK) {
                Ok(Chunk::Final(_)) => break,
                Ok(Chunk::Text(_)) => {}
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        self.unfinished = false;
    }

    /// Whether the process is still alive.
    ///
    /// Worth asking before relying on it. A session is long lived by design, so it has time
    /// to be killed by something else, and the failure otherwise arrives as a broken pipe
    /// halfway through a question somebody just asked out loud.
    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

/// How long a session gets to finish on its own after being asked to.
///
/// Closing stdin is the polite ask and it is worth making, because the transcript is what a
/// later `--resume` reads and killing mid write leaves it half done. But asking is not the
/// same as waiting, and the wait used to be unbounded. The session being given up on is
/// usually the one that has already blown a deadline, which is precisely the one that will
/// not answer, so the ask gets a bound and then the child is killed.
const GRACE: std::time::Duration = std::time::Duration::from_secs(2);

impl Session {
    /// Ends the session now rather than whenever the child decides.
    ///
    /// Safe to call more than once, which matters because `Drop` calls it too.
    pub fn stop(&mut self) {
        self.stdin.take();
        if !self.finished_within(GRACE) {
            self.kill_the_group();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }

    /// Ends the child and everything it started.
    ///
    /// The group id is the child's pid, because it was spawned as its own group leader.
    /// Signalling the group rather than the pid is what makes the stdout pipe actually close,
    /// since a surviving grandchild holds the write end otherwise.
    fn kill_the_group(&mut self) {
        let pid = self.child.id() as libc::pid_t;
        // Sound because the group was created by this process for this child, and it is only
        // signalled after the grace has expired and before the child is reaped, so the id
        // cannot yet have been reused.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }

    /// Whether the child ended on its own inside `grace`.
    fn finished_within(&mut self, grace: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + grace;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                // Either out of time, or the wait itself failed and waiting again will not
                // help. Both mean stop asking and start insisting.
                _ => return false,
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Bounded. This used to be a bare `child.wait()`, so dropping a session whose child
        // kept working blocked the dropping thread for as long as the child felt like, which
        // meant a deadline bought nothing and one stuck session stalled the voice loop. See
        // bug 13.
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub `claude` that reads the prompt, then keeps working and ignores EOF on stdin.
    ///
    /// That is the shape that matters. A child which exits when its stdin closes would hide
    /// the bug entirely, because the polite ask alone would be enough.
    fn stubborn_stub(dir: &Path, seconds: u64) -> std::path::PathBuf {
        let path = dir.join("claude-stubborn");
        std::fs::write(
            &path,
            format!("#!/bin/sh\ntrap '' HUP PIPE\ncat > /dev/null\nsleep {seconds}\n"),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// The bug. `Drop` closed stdin and then called `child.wait()` with no kill and no bound,
    /// and every path that gives up on an agent only drops the `Session`. So a deadline
    /// declared blown bought nothing: the caller blocked for as long as the child felt like
    /// carrying on, and in the pool one stuck session stalled the voice loop behind it.
    #[test]
    fn dropping_a_session_does_not_wait_for_a_child_that_will_not_stop() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Runner::at(stubborn_stub(dir.path(), 30));
        let id = SessionId::fresh().unwrap();
        let session = runner
            .open_session(&id, &dir.path().join("work"), "", false)
            .expect("the stub should start");

        let started = std::time::Instant::now();
        drop(session);
        let took = started.elapsed();

        // GRACE plus room for a slow machine, and far under the 30 seconds the child wanted.
        assert!(
            took < std::time::Duration::from_secs(10),
            "dropping took {took:?}, so it is still waiting for the child to decide"
        );
    }

    /// And the polite ask still has to happen, or the transcript a later resume reads is left
    /// half written. A child that does exit when its stdin closes must not be killed.
    #[test]
    fn a_session_that_finishes_on_its_own_is_not_killed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claude-polite");
        std::fs::write(&path, "#!/bin/sh\ncat > /dev/null\nexit 0\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let runner = Runner::at(&path);
        let id = SessionId::fresh().unwrap();
        let mut session = runner
            .open_session(&id, &dir.path().join("work"), "", false)
            .expect("the stub should start");

        // Closing stdin is what lets it finish, so the ask has to come first. It then ends on
        // its own well inside the grace and nothing has to insist.
        let started = std::time::Instant::now();
        session.stop();
        let took = started.elapsed();
        assert!(
            took < GRACE,
            "a child that exits when its stdin closes waited the full grace, took {took:?}"
        );
    }

    /// `Drop` calls `stop` too, so calling it explicitly first must not make the drop misbehave.
    #[test]
    fn stopping_twice_is_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Runner::at(stubborn_stub(dir.path(), 30));
        let id = SessionId::fresh().unwrap();
        let mut session = runner
            .open_session(&id, &dir.path().join("work"), "", false)
            .expect("the stub should start");

        session.stop();
        let started = std::time::Instant::now();
        drop(session);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    /// A session that cannot start is an error, not a handle to nothing. Everything
    /// downstream would otherwise believe it is talking to something.
    #[test]
    fn a_session_that_cannot_start_says_so() {
        let runner = Runner::at("/nonexistent/definitely-not-claude");
        let s = SessionId::fresh().unwrap();
        assert!(
            runner
                .open_session(&s, Path::new("/tmp"), "", false)
                .is_err()
        );
    }

    /// Pinning a new conversation and resuming an existing one are different flags, and
    /// sending both is an error. This is what makes a reopen after a crash carry on rather
    /// than silently start a second conversation.
    #[test]
    fn resuming_and_pinning_are_not_the_same_flag() {
        let runner = Runner::default();
        let s = SessionId::fresh().unwrap();

        // Built rather than run, since running needs a real claude and the argument shape is
        // the thing that decides whether a conversation survives a restart.
        for (resume, expected, forbidden) in [
            (false, "--session-id", "--resume"),
            (true, "--resume", "--session-id"),
        ] {
            let args = runner.session_args(&s, "", resume);
            assert!(args.contains(&expected.to_string()), "{args:?}");
            assert!(!args.contains(&forbidden.to_string()), "{args:?}");
        }
    }
}
