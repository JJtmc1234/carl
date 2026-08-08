//! Driving the `claude` command line in headless mode.
//!
//! There is no Rust binding for the Claude Agent SDK, so Carl runs the real `claude` binary
//! as a child process and reads its JSON. That is not a workaround. It is the same thing the
//! SDK does, minus a language runtime, and it means Carl supervises an agent as a process,
//! which is exactly what AOS is being built to do.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::{Error, Result, SessionId};

mod stream;
pub use stream::{Chunk, Flow, chunk_of};

/// One turn: what to say, and which conversation to say it in.
pub struct Turn<'a> {
    pub session: &'a SessionId,
    /// False on the very first message of a thread, because there is nothing to resume yet.
    pub resume: bool,
    pub prompt: &'a str,
    /// Memory and standing instructions, appended to Carl's system prompt.
    pub extra_system: Option<&'a str>,
    pub workdir: &'a Path,
}

/// What `claude --output-format json` gives back.
///
/// Only the fields Carl uses are named. Serde ignores the rest, so a new field in a future
/// Claude Code release does not break the parse.
#[derive(Debug, Deserialize)]
struct Envelope {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub text: String,
    /// True when Carl was talked over and stopped reading the rest.
    ///
    /// Worth carrying rather than dropping. Claude's own session holds the whole answer, so
    /// without this the record would claim Carl said things nobody ever heard.
    pub interrupted: bool,
    /// The session Claude Code actually used. Compared against the one we asked for, because
    /// a silent mismatch means the next turn resumes the wrong conversation.
    pub session_id: Option<String>,
    pub cost_usd: Option<f64>,
}

/// Where the `claude` binary lives, so tests can point at a stand in.
pub struct Runner {
    program: PathBuf,
}

impl Default for Runner {
    fn default() -> Self {
        Self {
            program: PathBuf::from("claude"),
        }
    }
}

impl Runner {
    pub fn at(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }

    pub fn args_for(&self, turn: &Turn<'_>) -> Vec<String> {
        self.args_with(turn, ["--print", "--output-format", "json"])
    }

    fn args_with<'b>(
        &self,
        turn: &Turn<'_>,
        head: impl IntoIterator<Item = &'b str>,
    ) -> Vec<String> {
        let mut args: Vec<String> = head.into_iter().map(str::to_owned).collect();

        // --session-id pins a new conversation to an id we chose. --resume continues one that
        // already exists. Sending both is an error, which is why `resume` is a flag on the
        // turn rather than something guessed here.
        if turn.resume {
            args.push("--resume".into());
            args.push(turn.session.to_string());
        } else {
            args.push("--session-id".into());
            args.push(turn.session.to_string());
        }

        if let Some(system) = turn.extra_system {
            args.push("--append-system-prompt".into());
            args.push(system.to_string());
        }

        args
    }

    pub fn ask(&self, turn: &Turn<'_>) -> Result<Answer> {
        use std::io::Write;

        std::fs::create_dir_all(turn.workdir)?;

        let mut child = Command::new(&self.program)
            .args(self.args_for(turn))
            .current_dir(turn.workdir)
            // The prompt goes on stdin rather than the argument vector. A Slack message can
            // be longer than the argument limit, and it can contain anything at all.
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

        let out = child.wait_with_output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

        if !out.status.success() {
            return Err(Error::Claude(format!(
                "claude exited with {}: {}",
                out.status,
                first_line(&stderr).unwrap_or("no error output")
            )));
        }

        parse(&stdout)
    }
}

/// Pulls the answer out of the JSON envelope.
pub fn parse(stdout: &str) -> Result<Answer> {
    let envelope: Envelope = serde_json::from_str(stdout.trim()).map_err(|e| {
        Error::Claude(format!(
            "cannot read the answer as JSON ({e}): {}",
            first_line(stdout).unwrap_or("<empty>")
        ))
    })?;

    // An error envelope still parses, so checking the flag matters more than checking that
    // the JSON was well formed.
    if envelope.is_error {
        return Err(Error::Claude(
            envelope
                .result
                .unwrap_or_else(|| "claude reported an error".into()),
        ));
    }

    let text = envelope
        .result
        .ok_or_else(|| Error::Claude("the answer had no result field".into()))?;

    Ok(Answer {
        text,
        interrupted: false,
        session_id: envelope.session_id,
        cost_usd: envelope.total_cost_usd,
    })
}

fn first_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> SessionId {
        SessionId::fresh().unwrap()
    }

    #[test]
    fn a_first_message_pins_the_session_and_does_not_resume() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: false,
            prompt: "hi",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        assert!(args.contains(&"--session-id".to_string()), "{args:?}");
        assert!(!args.contains(&"--resume".to_string()), "{args:?}");
        assert!(args.contains(&s.to_string()));
    }

    /// Sending both flags is an error, so the two paths must stay exclusive.
    #[test]
    fn a_later_message_resumes_and_does_not_re_pin() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: true,
            prompt: "and another thing",
            extra_system: None,
            workdir: Path::new("/tmp"),
        });
        assert!(args.contains(&"--resume".to_string()), "{args:?}");
        assert!(!args.contains(&"--session-id".to_string()), "{args:?}");
    }

    #[test]
    fn memory_is_appended_to_the_system_prompt() {
        let s = session();
        let args = Runner::default().args_for(&Turn {
            session: &s,
            resume: true,
            prompt: "hi",
            extra_system: Some("JJ is 11."),
            workdir: Path::new("/tmp"),
        });
        let at = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(args[at + 1], "JJ is 11.");
    }

    #[test]
    fn a_normal_answer_is_read() {
        let answer = parse(
            r#"{"type":"result","result":"Hello JJ","session_id":"abc","total_cost_usd":0.01}"#,
        )
        .unwrap();
        assert_eq!(answer.text, "Hello JJ");
        assert_eq!(answer.session_id.as_deref(), Some("abc"));
        assert_eq!(answer.cost_usd, Some(0.01));
    }

    /// An error envelope is valid JSON, so parsing successfully is not the same as succeeding.
    #[test]
    fn an_error_envelope_is_an_error() {
        let err = parse(r#"{"is_error":true,"result":"session not found"}"#).unwrap_err();
        assert!(err.to_string().contains("session not found"), "{err}");
    }

    #[test]
    fn unknown_fields_do_not_break_the_parse() {
        let answer =
            parse(r#"{"result":"fine","session_id":"x","some_future_field":{"nested":[1,2]}}"#)
                .unwrap();
        assert_eq!(answer.text, "fine");
    }

    #[test]
    fn non_json_output_says_what_it_actually_got() {
        let err = parse("command not found: claude").unwrap_err();
        assert!(err.to_string().contains("command not found"), "{err}");
    }

    #[test]
    fn an_empty_answer_is_an_error_rather_than_an_empty_reply() {
        assert!(parse("").is_err());
        assert!(parse(r#"{"type":"result"}"#).is_err(), "no result field");
    }
}
