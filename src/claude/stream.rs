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

use super::{Answer, Runner, Turn, parse};
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

            return Ok(Answer {
                text: said,
                interrupted: true,
                session_id: envelope.and_then(|a| a.session_id),
                cost_usd: None,
            });
        }

        let _ = reader.join();
        let status = child.wait()?;

        if let Some(answer) = envelope {
            return Ok(answer);
        }

        // No final envelope. Either the child died or it printed something unexpected, and
        // the collected text is the only evidence of which.
        let mut why = String::new();
        if let Some(mut err) = child.stderr.take() {
            use std::io::Read;
            let _ = err.read_to_string(&mut why);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
