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

use super::{Answer, Chunk, Flow, Runner, Say, chunk_of};
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

        // The same three the one shot path passes, and for the same reasons.
        //
        // This list is built from scratch rather than through `args_with`, so every flag added
        // there had to be added here too and none of them were. The long running agents under
        // the supervisor are the ones that matter most: they ran on whatever model the CLI
        // defaulted to while their folder said otherwise, could not open the shared memory they
        // are told to read first, and still held the subagent tool that delegation refuses.
        if let Some(model) = &self.model {
            args.push("--model".into());
            args.push(model.clone());
        }
        if let Some(shared) = super::shared_memory() {
            args.push("--add-dir".into());
            args.push(shared);
        }
        args.push("--disallowedTools".into());
        args.extend(super::NEVER.iter().map(|s| (*s).to_string()));

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
        on_text: &mut dyn FnMut(Say<'_>) -> Flow,
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
                // Said out loud rather than swallowed. The person who can widen the allow list
                // is the one reading this, and until now the refusal never left the transcript.
                Ok(Chunk::Refused { tool, why }) => {
                    if on_text(Say::Refused {
                        tool: &tool,
                        why: &why,
                    }) == Flow::Stop
                    {
                        break;
                    }
                }
                // Shown as it happens, and deliberately not added to `said`: it is a note about
                // working, not part of the answer, and it must not end up in the transcript or
                // be spoken out loud.
                Ok(Chunk::Doing { tool, detail }) => {
                    if on_text(Say::Doing {
                        tool: &tool,
                        detail: &detail,
                    }) == Flow::Stop
                    {
                        break;
                    }
                }
                // Shown as it happens and kept out of `said` for the same reason as a tool
                // note. Reasoning is not the reply.
                Ok(Chunk::Thinking(t)) => {
                    if on_text(Say::Thinking(&t)) == Flow::Stop {
                        break;
                    }
                }
                Ok(Chunk::Text(t)) => {
                    said.push_str(&t);
                    if on_text(Say::Words(&t)) == Flow::Stop {
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
                // Nobody is listening to this turn any more, so a refusal in it is history.
                Ok(Chunk::Refused { .. }) | Ok(Chunk::Doing { .. }) | Ok(Chunk::Thinking(_)) => {
                    continue;
                }
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

    /// Which process this is.
    ///
    /// Only a supervisor has any business with this. Everything else here talks to a session
    /// through its pipes and would be worse for knowing a pid.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The exit code, once there is one.
    ///
    /// `alive` asks the same question and throws the answer away. A supervisor deciding whether
    /// to restart wants the code, because a process killed by a signal and one that chose to
    /// exit are the same shape of event and not the same thing.
    pub fn ended(&mut self) -> Option<Option<i32>> {
        match self.child.try_wait() {
            Ok(Some(status)) => Some(status.code()),
            _ => None,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Closing stdin first asks it to finish. Killing without that leaves the transcript
        // half written, and the transcript is what a later `--resume` reads.
        self.stdin.take();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

#[cfg(test)]
mod session_arg_tests {
    use super::*;
    use crate::claude::{NEVER, Runner};

    fn args(runner: &Runner, resume: bool) -> Vec<String> {
        let session = SessionId::fresh().expect("a session id");
        runner.session_args(&session, "brief", resume)
    }

    /// The bug this covers. `session_args` builds its list from scratch, so every flag added to
    /// `args_with` had to be added here too and none of them were. The agents this path starts
    /// are the long running ones, which are the agents that matter most.
    #[test]
    fn a_session_runs_on_the_model_its_folder_asks_for() {
        let a = args(&Runner::at("claude").running("claude-fable-5"), false);
        let at = a
            .iter()
            .position(|x| x == "--model")
            .expect("the model must reach a long running agent too");
        assert_eq!(a[at + 1], "claude-fable-5");
    }

    #[test]
    fn an_unset_model_passes_no_flag_on_the_session_path_either() {
        assert!(
            !args(&Runner::at("claude"), false)
                .iter()
                .any(|x| x == "--model")
        );
    }

    /// A long running agent is told to read the shared memory before it works, and could not
    /// open it, because this path never passed the directory.
    #[test]
    fn a_session_can_reach_the_shared_memory_when_there_is_one() {
        let a = args(&Runner::at("claude"), false);
        match super::super::shared_memory() {
            Some(dir) => {
                let at = a.iter().position(|x| x == "--add-dir").expect("--add-dir");
                assert_eq!(a[at + 1], dir);
            }
            None => assert!(!a.iter().any(|x| x == "--add-dir")),
        }
    }

    /// Delegation refuses a spawned subagent, and this path handed one to every agent it started.
    #[test]
    fn a_session_never_holds_the_subagent_tool() {
        for resume in [true, false] {
            let a = args(&Runner::at("claude"), resume);
            let at = a
                .iter()
                .position(|x| x == "--disallowedTools")
                .expect("the refusal must be on the session path");
            for banned in NEVER {
                assert!(
                    a[at + 1..].iter().any(|x| x == banned),
                    "{banned} not refused"
                );
            }
        }
    }

    /// Resuming and pinning are different flags and sending both is an error.
    #[test]
    fn resuming_and_pinning_are_never_sent_together() {
        let resumed = args(&Runner::at("claude"), true);
        let fresh = args(&Runner::at("claude"), false);
        assert!(resumed.iter().any(|x| x == "--resume"));
        assert!(!resumed.iter().any(|x| x == "--session-id"));
        assert!(fresh.iter().any(|x| x == "--session-id"));
        assert!(!fresh.iter().any(|x| x == "--resume"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
