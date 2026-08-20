//! Reading an answer as it is written, rather than waiting for the last word.
//!
//! Claude is the slow part of a spoken exchange by a wide margin. Measured here, the whole
//! answer takes five to twenty five seconds and the first words take under one. Waiting for
//! the last word before saying the first throws away everything in between, and a machine
//! that pauses twenty seconds and then talks does not feel like a conversation.
//!
//! `--output-format stream-json` emits one JSON object per line as the answer is generated.
//! Only two kinds matter: a text delta, and the final envelope.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};

use super::{Answer, Runner, Turn, check, parse};
use crate::{Error, Result};

/// A piece of an answer as it arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    /// More words.
    Text(String),
    /// The final envelope, which carries the session id and the cost.
    Final(Box<Answer>),
}

/// What the listener wants to happen next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    /// Stop reading. Used when Carl is talked over and the rest of the answer is moot.
    Stop,
}

impl Runner {
    /// Runs a turn, handing each piece of text to `on_text` as it arrives.
    ///
    /// `on_text` may block for as long as it likes, which it does, because speaking a
    /// sentence takes about as long as saying it. A reader thread keeps draining the child's
    /// output throughout, so Claude carries on writing at full speed while Carl talks. Reading
    /// on this thread instead would stall Claude for the length of every sentence spoken.
    pub fn ask_streaming(
        &self,
        turn: &Turn<'_>,
        on_text: &mut dyn FnMut(&str) -> Flow,
    ) -> Result<Answer> {
        check(turn)?;
        std::fs::create_dir_all(turn.workdir)?;

        let mut child = Command::new(&self.program)
            .args(self.args_with(
                turn,
                [
                    "--print",
                    "--output-format",
                    "stream-json",
                    "--include-partial-messages",
                    "--verbose",
                ],
            ))
            .current_dir(turn.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| Error::Claude(format!("cannot run {}: {e}", self.program.display())))?;

        child
            .stdin
            .take()
            .ok_or_else(|| Error::Claude("no stdin on the child".into()))?
            .write_all(turn.prompt.as_bytes())?;

        let out = child
            .stdout
            .take()
            .ok_or_else(|| Error::Claude("no stdout on the child".into()))?;

        // Drained from the start, on its own thread, and this is not tidiness.
        //
        // A pipe holds about 64 KiB. Once it is full the writer blocks, and a child blocked
        // writing stderr stops writing stdout as well, so the reader below never sees EOF and
        // this function never leaves its receive loop. `claude --verbose` reaches 64 KiB of
        // node warnings and MCP chatter without trying. That is bug 1 in bug-list.md again,
        // in a different file: a pipe nobody drains. See bug 11.
        //
        // Kept rather than thrown away, because the text is the only evidence of what went
        // wrong when the child dies without a final envelope. Bounded, because a chatty child
        // must not be able to trade a deadlock for eating memory instead.
        let errors = child
            .stderr
            .take()
            .ok_or_else(|| Error::Claude("no stderr on the child".into()))?;
        let errors = std::thread::spawn(move || drain(errors));

        let (tx, rx): (_, Receiver<Chunk>) = channel();
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(out)
                .lines()
                .map_while(std::result::Result::ok)
            {
                if let Some(chunk) = chunk_of(&line)
                    && tx.send(chunk).is_err()
                {
                    // Nobody is listening any more, which means Carl was interrupted.
                    break;
                }
            }
        });

        let mut said = String::new();
        let mut envelope = None;
        let mut stopped = false;

        for chunk in &rx {
            match chunk {
                Chunk::Text(t) => {
                    said.push_str(&t);
                    if on_text(&t) == Flow::Stop {
                        stopped = true;
                        break;
                    }
                }
                Chunk::Final(a) => envelope = Some(*a),
            }
        }

        if stopped {
            // Killed rather than waited for. The answer is already irrelevant, and leaving
            // Claude running would hold the session open against the next question.
            let _ = child.kill();
            let _ = child.wait();
            drop(rx);
            let _ = reader.join();
            let _ = errors.join();

            return Ok(Answer {
                text: said,
                interrupted: true,
                session_id: envelope.and_then(|a| a.session_id),
                cost_usd: None,
            });
        }

        let _ = reader.join();
        let status = child.wait()?;
        // Joined after the wait, so the pipe has certainly reached EOF.
        let why = errors.join().unwrap_or_default();

        if let Some(answer) = envelope {
            return Ok(answer);
        }

        // No final envelope. Either the child died or it printed something unexpected, and
        // the collected text is the only evidence of which.
        Err(Error::Claude(format!(
            "claude exited with {status} before finishing: {}",
            why.lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or(if said.is_empty() { "no output" } else { &said })
        )))
    }
}

/// Reads one line of the stream. `None` for the many lines Carl does not care about.
pub fn chunk_of(line: &str) -> Option<Chunk> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;

    match v.get("type")?.as_str()? {
        // The final envelope has exactly the shape the non streaming mode returns, so it is
        // parsed by the same code. One definition of what an answer is.
        "result" => parse(line).ok().map(|a| Chunk::Final(Box::new(a))),
        "stream_event" => {
            let event = v.get("event")?;
            if event.get("type")?.as_str()? != "content_block_delta" {
                return None;
            }
            let delta = event.get("delta")?;
            if delta.get("type")?.as_str()? != "text_delta" {
                return None;
            }
            Some(Chunk::Text(delta.get("text")?.as_str()?.to_owned()))
        }
        _ => None,
    }
}

/// Reads a pipe to the end, keeping only the first `KEEP` bytes.
///
/// Reading to the end is what matters: stopping early lets the pipe fill and blocks the child,
/// which is the bug this exists to prevent. Keeping only the start is what stops a child that
/// writes megabytes of warnings from trading that deadlock for unbounded memory. The first
/// lines are the useful ones anyway, since that is where a failure says what it was.
fn drain(mut pipe: impl std::io::Read) -> String {
    const KEEP: usize = 8 * 1024;

    let mut kept = Vec::new();
    let mut buf = [0u8; 8 * 1024];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if kept.len() < KEEP {
                    let room = KEEP - kept.len();
                    kept.extend_from_slice(&buf[..n.min(room)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub `claude` that writes `noise` bytes to stderr and then a valid answer.
    ///
    /// A shell script rather than a Rust helper binary, because what is being tested is a real
    /// process on the other end of a real pipe. Anything short of that does not have the pipe.
    fn stub(dir: &std::path::Path, noise: usize) -> std::path::PathBuf {
        let path = dir.join("claude-stub");
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\n\
                 cat > /dev/null\n\
                 head -c {noise} /dev/zero | tr '\\0' 'e' >&2\n\
                 printf '%s\\n' '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{{\"type\":\"text_delta\",\"text\":\"hello\"}}}}}}'\n\
                 printf '%s\\n' '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"hello\",\"session_id\":\"s1\",\"total_cost_usd\":0.0}}'\n"
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Runs one streamed turn against the stub, on a thread, so a hang is a failed test rather
    /// than a suite that never finishes. This project has hung a whole run on exactly this
    /// mistake before, which is why the deadline is here and not left to the harness.
    fn answer_within(noise: usize, secs: u64) -> Option<Result<Answer>> {
        let dir = tempfile::tempdir().unwrap();
        let program = stub(dir.path(), noise);
        let work = dir.path().join("work");

        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let runner = Runner::at(program);
            let session = crate::SessionId::fresh().unwrap();
            let turn = Turn {
                session: &session,
                resume: false,
                prompt: "hi",
                extra_system: None,
                workdir: &work,
            };
            let _ = tx.send(runner.ask_streaming(&turn, &mut |_| Flow::Continue));
        });

        // The tempdir has to outlive the child, so it is held until the answer arrives.
        let got = rx.recv_timeout(std::time::Duration::from_secs(secs)).ok();
        drop(dir);
        got
    }

    /// The bug. stderr was piped and not read until after the child had exited. A pipe holds
    /// about 64 KiB, and once it is full the writer blocks. A child blocked writing stderr
    /// stops writing stdout too, so the reader thread never sees EOF and the parent sits in
    /// its receive loop forever.
    ///
    /// This is bug 1 in this list again, in a different file. There it was the agent's output
    /// pipe, here it is the child's stderr, and both are a pipe nobody drains.
    #[test]
    fn a_chatty_stderr_does_not_deadlock_the_answer() {
        // Well past the 64 KiB the pipe holds. `claude --verbose` node warnings and MCP
        // server chatter reach this easily.
        let got = answer_within(200_000, 20);

        let answer = got
            .expect("the answer never arrived, so a full stderr pipe is still deadlocking it")
            .expect("the stub produced a valid answer");
        assert_eq!(answer.text, "hello");
        assert_eq!(answer.session_id.as_deref(), Some("s1"));
    }

    /// The same path with a stderr small enough to fit in the pipe, which is what used to work
    /// and must keep working.
    #[test]
    fn a_quiet_stderr_still_answers() {
        let answer = answer_within(1_000, 20)
            .expect("no answer")
            .expect("valid answer");
        assert_eq!(answer.text, "hello");
    }

    #[test]
    fn a_text_delta_is_the_words() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" automation science"}}}"#;
        assert_eq!(
            chunk_of(line),
            Some(Chunk::Text(" automation science".into()))
        );
    }

    #[test]
    fn the_result_line_carries_the_whole_answer() {
        let line = r#"{"type":"result","subtype":"success","result":"Research automation.","session_id":"abc","total_cost_usd":0.1}"#;
        match chunk_of(line) {
            Some(Chunk::Final(a)) => {
                assert_eq!(a.text, "Research automation.");
                assert_eq!(a.session_id.as_deref(), Some("abc"));
                assert!(!a.interrupted);
            }
            other => panic!("expected the final envelope, got {other:?}"),
        }
    }

    /// The stream is mostly lines Carl has no use for. Every one of them must be skipped
    /// rather than mistaken for words to say out loud.
    #[test]
    fn the_noise_in_the_stream_is_ignored() {
        for line in [
            r#"{"type":"system","subtype":"init","tools":["Bash"]}"#,
            r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"whole thing"}]}}"#,
            "",
            "not json at all",
        ] {
            assert_eq!(chunk_of(line), None, "should have ignored {line}");
        }
    }

    /// The assistant line repeats the entire answer so far. Treating it as text would say
    /// every sentence twice, which is the obvious way to get this wrong.
    #[test]
    fn the_repeated_assistant_line_is_not_spoken_again() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Research automation science next."}]}}"#;
        assert_eq!(chunk_of(line), None);
    }

    /// A thinking delta is Claude working, not Claude answering.
    #[test]
    fn thinking_is_not_read_out_loud() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hmm"}}}"#;
        assert_eq!(chunk_of(line), None);
    }

    /// An error envelope arrives on the same result line, so it must still be caught.
    #[test]
    fn an_error_envelope_produces_no_chunk_rather_than_a_false_answer() {
        let line = r#"{"type":"result","is_error":true,"result":"session not found"}"#;
        assert_eq!(chunk_of(line), None);
    }
}
