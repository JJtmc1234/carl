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

/// One piece of a turn on its way to whoever is watching, and what kind of piece it is.
///
/// Words, reasoning and tool calls all arrive on the same channel and are emphatically not the
/// same thing. The speakers want only the words. A screen wants all three, told apart, because
/// the whole value of the other two is that they look different from the answer.
///
/// Before this existed the channel was a bare `&str` and the working notes were formatted into
/// it. That reads fine in a terminal and is wrong everywhere else: the voice had no way to know
/// it was about to read a file path out loud, and the panel had no way to style a note as
/// anything other than more of Carl's sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say<'a> {
    /// The answer itself. The only kind that belongs in the transcript or in the speakers.
    Words(&'a str),
    /// Carl working out what to say.
    ///
    /// Never spoken and never added to the answer. It is not addressed to anybody, so reading
    /// it out loud is both wrong and roughly twice as long as the reply.
    Thinking {
        text: &'a str,
        /// Roughly how many tokens of reasoning, when the CLI says. The text is usually
        /// redacted, so this is often the only thing there is to show.
        tokens: Option<u32>,
    },
    /// A tool he has just picked up.
    Doing { tool: &'a str, detail: &'a str },
    /// A tool call refused for want of permission.
    Refused { tool: &'a str, why: &'a str },
}

impl Chunk {
    /// The same piece, borrowed, for anything that takes the streaming form.
    ///
    /// The two shapes exist for a reason: `Chunk` owns its text because it crosses a channel,
    /// and `Say` borrows because a surface only looks at it. A path that reads chunks itself
    /// still wants everything written for `Say`, and converting by hand at each of those was
    /// how one of them came to show tool calls while another silently did not.
    ///
    /// `Final` has no borrowed form. It is the envelope repeating the whole answer rather than
    /// a piece of one, and the pieces have already been handed over by the time it arrives.
    pub fn as_say(&self) -> Option<Say<'_>> {
        match self {
            Chunk::Text(t) => Some(Say::Words(t)),
            Chunk::Thinking { text, tokens } => Some(Say::Thinking {
                text,
                tokens: *tokens,
            }),
            Chunk::Doing { tool, detail } => Some(Say::Doing { tool, detail }),
            Chunk::Refused { tool, why } => Some(Say::Refused { tool, why }),
            Chunk::Final(_) => None,
        }
    }
}

impl Say<'_> {
    /// The words, if this is words. `None` for every kind of note.
    ///
    /// The one call a surface that only wants the answer has to make, so forgetting to filter
    /// is a compile error rather than Carl narrating his own reasoning.
    pub fn words(&self) -> Option<&str> {
        match self {
            Say::Words(t) => Some(t),
            _ => None,
        }
    }
}

/// A piece of an answer as it arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    /// More words.
    Text(String),
    /// Reasoning, as it is produced.
    ///
    /// **The text is usually empty and that is not a bug.** The CLI emits the thinking events
    /// with the content redacted: `{"type":"thinking_delta","thinking":"","estimated_tokens":50}`.
    /// So what is actually available is that thinking is happening and roughly how much of it,
    /// which is worth showing. An empty frame with the count thrown away looks like nothing is
    /// happening, which is the opposite of the truth and is what it looked like before.
    Thinking {
        text: String,
        /// Roughly how many tokens of reasoning, when the CLI says.
        tokens: Option<u32>,
    },
    /// The final envelope, which carries the session id and the cost.
    Final(Box<Answer>),
    /// A tool call that was refused for want of permission.
    ///
    /// Headless has nobody to ask, so this is not a prompt somebody missed, it is a decision
    /// already taken. Carried out of the stream because otherwise the only party who knows is
    /// Carl, who then spends a turn explaining that he could not do the thing. The person who
    /// can actually fix it, by widening `permissions.json`, never hears about it at all.
    Refused {
        /// The tool as the CLI named it, so it can be pasted into the allow list.
        tool: String,
        why: String,
    },
    /// A tool Carl has just picked up.
    ///
    /// Carried so a person watching can tell a long answer from a stuck one. Without it the
    /// only thing on screen between the question and the first word is a caret, and a Carl
    /// reading forty files looks exactly like a Carl that has wedged. What he is doing is the
    /// difference, and he already says it: it was being thrown away here.
    Doing {
        /// `Bash`, `Read`, `Grep`, as the CLI names it.
        tool: String,
        /// The part worth reading: the command, or the path. Empty when the call carries none.
        detail: String,
    },
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
        on_text: &mut dyn FnMut(Say<'_>) -> Flow,
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
                // Put in front of whoever is watching, for the same reason as in a session: the
                // only person who can widen the allow list is the one reading the answer.
                Chunk::Refused { tool, why } => {
                    if on_text(Say::Refused {
                        tool: &tool,
                        why: &why,
                    }) == Flow::Stop
                    {
                        break;
                    }
                }
                // Shown, never kept. A note about working is not part of the answer.
                Chunk::Doing { tool, detail } => {
                    if on_text(Say::Doing {
                        tool: &tool,
                        detail: &detail,
                    }) == Flow::Stop
                    {
                        break;
                    }
                }
                // Same rule as a tool note. Reasoning is shown and never kept, so an
                // interrupted answer does not have Carl's working in the transcript.
                Chunk::Thinking { text, tokens } => {
                    if on_text(Say::Thinking {
                        text: &text,
                        tokens,
                    }) == Flow::Stop
                    {
                        break;
                    }
                }
                Chunk::Text(t) => {
                    said.push_str(&t);
                    if on_text(Say::Words(&t)) == Flow::Stop {
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
            // Two delta kinds are worth having and they carry their payload under different
            // keys. `thinking_delta` holds `thinking`, `text_delta` holds `text`.
            match delta.get("type")?.as_str()? {
                "text_delta" => Some(Chunk::Text(delta.get("text")?.as_str()?.to_owned())),
                "thinking_delta" => Some(Chunk::Thinking {
                    text: delta.get("thinking")?.as_str()?.to_owned(),
                    tokens: delta
                        .get("estimated_tokens")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                }),
                _ => None,
            }
        }
        // What Carl has picked up. The assistant line repeats the whole answer so far, which is
        // why its text is deliberately ignored, but its tool calls are new each time and are
        // the only sign of progress there is while he works.
        "assistant" => doing(&v),
        // A refusal arrives as the result of a tool call rather than as an event of its own,
        // which is why nothing here saw it before: this branch did not exist and the line was
        // one of "the many lines Carl does not care about".
        "user" => refusal(&v),
        _ => None,
    }
}

/// The tool Carl has just picked up, if this line carries one.
///
/// The first `tool_use` block only. A line can carry several, and listing them all turns a
/// progress note into a wall, which is the thing it exists to be an alternative to.
fn doing(v: &serde_json::Value) -> Option<Chunk> {
    let content = v.get("message")?.get("content")?.as_array()?;
    for block in content {
        if block.get("type")?.as_str()? != "tool_use" {
            continue;
        }
        let tool = block.get("name")?.as_str()?.to_owned();
        let input = block.get("input");
        // The one field worth reading, by the name each tool actually uses for it.
        let detail = input
            .and_then(|i| {
                for key in [
                    "command",
                    "file_path",
                    "pattern",
                    "path",
                    "prompt",
                    "description",
                ] {
                    if let Some(v) = i.get(key).and_then(|v| v.as_str()) {
                        return Some(v.to_owned());
                    }
                }
                None
            })
            .unwrap_or_default();
        return Some(Chunk::Doing { tool, detail });
    }
    None
}

/// A tool result that says the call was refused, if that is what this is.
///
/// Matched on the wording the CLI uses rather than on a flag, because `is_error` is also set by
/// a command that ran and failed, and those are Carl's problem rather than JJ's.
fn refusal(v: &serde_json::Value) -> Option<Chunk> {
    let content = v.get("message")?.get("content")?.as_array()?;
    for block in content {
        if block.get("type")?.as_str()? != "tool_result" {
            continue;
        }
        let text = match block.get("content") {
            Some(serde_json::Value::String(t)) => t.clone(),
            Some(serde_json::Value::Array(parts)) => parts
                .iter()
                .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };
        let lower = text.to_lowercase();
        let denied = lower.contains("permission")
            || lower.contains("requested permissions")
            || lower.contains("has not been granted")
            || lower.contains("not allowed");
        if !denied {
            continue;
        }
        // The tool name, so it can be pasted into the allow list rather than worked out.
        //
        // This used to match `Bash(`, `Write`, `Read` and `Edit` and nothing else, so every
        // other tool came out as the literal words "a tool" and the refusal then told JJ to
        // add "a tool" to permissions.json. On 2026 09 02 Carl was refused `ListAgents` and
        // that is exactly what he was shown.
        //
        // The CLI's own wording is "requested permissions to use X", so the word after `use`
        // is taken first. The shape check is the fallback for wording that does not say it.
        let tool = named_tool(&text).unwrap_or_else(|| "a tool".to_string());
        return Some(Chunk::Refused { tool, why: text });
    }
    None
}


/// The tool a refusal is about, taken from the CLI's own wording.
///
/// Two ways round, because the wording has changed before. `use X` is what it says today. The
/// shape check behind it accepts anything that looks like a tool name: a capitalised word, a
/// `Bash(...)` pattern, or an `mcp__` name, which is what every tool in this system is called.
///
/// Returns nothing rather than a guess when neither finds one. A refusal naming the wrong tool
/// sends somebody to widen a permission that was never the problem.
fn named_tool(text: &str) -> Option<String> {
    let looks_like_one = |word: &str| {
        let word = word.trim_end_matches([',', '.', ':', ';']);
        if word.is_empty() {
            return None;
        }
        let shaped = word.starts_with("Bash(")
            || word.starts_with("mcp__")
            || (word.starts_with(|c: char| c.is_ascii_uppercase())
                && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        shaped.then(|| word.to_string())
    };

    let words: Vec<&str> = text.split_whitespace().collect();
    // `requested permissions to use ListAgents, but ...`
    for pair in words.windows(2) {
        if pair[0] == "use"
            && let Some(found) = looks_like_one(pair[1])
        {
            return Some(found);
        }
    }
    words.iter().find_map(|w| looks_like_one(w))
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
    ///
    /// It used to be discarded here, which meant no surface could show it even when showing it
    /// was the point. It is carried now, as its own kind, and the promise that it is never
    /// spoken is kept where the speaking happens rather than by throwing the data away.
    #[test]
    fn thinking_is_carried_as_its_own_kind() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"hmm"}}}"#;
        assert_eq!(
            chunk_of(line),
            Some(Chunk::Thinking {
                text: "hmm".into(),
                tokens: None
            })
        );
    }

    /// The two delta kinds keep their payload under different keys, and reading the wrong one
    /// yields nothing rather than the other one's text.
    #[test]
    fn a_thinking_delta_is_not_confused_with_a_text_delta() {
        let thinking = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"working"}}}"#;
        let words = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"answer"}}}"#;
        assert_eq!(
            chunk_of(thinking),
            Some(Chunk::Thinking {
                text: "working".into(),
                tokens: None
            })
        );
        assert_eq!(chunk_of(words), Some(Chunk::Text("answer".into())));
    }

    /// Only the words belong to the answer. Everything else has to be filtered by a caller,
    /// and `words()` is the one call that does it.
    #[test]
    fn only_words_count_as_the_answer() {
        assert_eq!(Say::Words("hello").words(), Some("hello"));
        assert_eq!(
            Say::Thinking {
                text: "hmm",
                tokens: None
            }
            .words(),
            None
        );
        assert_eq!(
            Say::Doing {
                tool: "Bash",
                detail: "ls"
            }
            .words(),
            None
        );
        assert_eq!(
            Say::Refused {
                tool: "Bash",
                why: "no"
            }
            .words(),
            None
        );
    }

    /// An error envelope arrives on the same result line, so it must still be caught.
    #[test]
    fn an_error_envelope_produces_no_chunk_rather_than_a_false_answer() {
        let line = r#"{"type":"result","is_error":true,"result":"session not found"}"#;
        assert_eq!(chunk_of(line), None);
    }
}

#[cfg(test)]
mod redacted_thinking_tests {
    use super::*;

    /// What the CLI actually sends, measured rather than assumed. The reasoning text is
    /// redacted and the size comes with it, so a parser that keeps only the text carries an
    /// empty string and throws away the one fact there is.
    #[test]
    fn a_redacted_thought_keeps_its_size() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"","estimated_tokens":50}}}"#;
        assert_eq!(
            chunk_of(line),
            Some(Chunk::Thinking {
                text: String::new(),
                tokens: Some(50)
            })
        );
    }

    /// A null size is absent rather than zero. Zero would read as "thought about nothing",
    /// which is a different claim from "did not say".
    #[test]
    fn a_missing_size_is_absent_rather_than_zero() {
        let line = r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"","estimated_tokens":null}}}"#;
        assert_eq!(
            chunk_of(line),
            Some(Chunk::Thinking {
                text: String::new(),
                tokens: None
            })
        );
    }

    /// Reasoning is still never part of the answer, whatever it carries.
    #[test]
    fn a_thought_is_never_words() {
        assert_eq!(
            Say::Thinking {
                text: "",
                tokens: Some(50)
            }
            .words(),
            None
        );
    }
}

#[cfg(test)]
mod refusal_tests {
    use super::*;

    /// The line JJ never saw. Taken from what the CLI actually emits when a tool is not
    /// permitted, rather than from a guess at its wording.
    #[test]
    fn a_refused_tool_comes_out_of_the_stream() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"x","is_error":true,"content":"Claude requested permissions to use Bash(python3:*), but you have not granted it yet."}]}}"#;
        match chunk_of(line) {
            Some(Chunk::Refused { tool, why }) => {
                assert_eq!(tool, "Bash(python3:*)", "the name has to be pasteable");
                assert!(why.contains("permissions"));
            }
            other => panic!("a refusal was not recognised: {other:?}"),
        }
    }

    /// A command that ran and failed is Carl's problem, not a permission JJ has to widen, and
    /// telling him to edit permissions.json would send him to fix the wrong thing.
    #[test]
    fn a_command_that_merely_failed_is_not_a_refusal() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"x","is_error":true,"content":"python3: can't open file 'missing.py': No such file or directory"}]}}"#;
        assert!(
            chunk_of(line).is_none(),
            "an ordinary failure is not a refusal"
        );
    }

    #[test]
    fn a_successful_tool_result_says_nothing() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"x","content":"42"}]}}"#;
        assert!(chunk_of(line).is_none());
    }

    /// The bug JJ hit on 2026 09 02. Carl was refused `ListAgents` and the message said
    /// `[refused: a tool]` and told him to add `"a tool"` to permissions.json, which is not
    /// something anybody can act on. The extractor matched `Bash(`, `Write`, `Read` and `Edit`
    /// and nothing else.
    #[test]
    fn a_refusal_names_a_tool_that_is_not_bash_or_a_file_tool() {
        for (wording, expected) in [
            (
                "Claude requested permissions to use ListAgents, but you have not granted it yet.",
                "ListAgents",
            ),
            (
                "Claude requested permissions to use ToolSearch, but you have not granted it yet.",
                "ToolSearch",
            ),
            (
                "Claude requested permissions to use mcp__claude_ai_Gmail__send_message, but you have not granted it yet.",
                "mcp__claude_ai_Gmail__send_message",
            ),
            (
                "Claude requested permissions to use Bash(python3:*), but you have not granted it yet.",
                "Bash(python3:*)",
            ),
        ] {
            let line = format!(
                r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"x","is_error":true,"content":"{wording}"}}]}}}}"#
            );
            match chunk_of(&line) {
                Some(Chunk::Refused { tool, .. }) => assert_eq!(tool, expected, "{wording}"),
                other => panic!("not recognised as a refusal: {other:?}"),
            }
        }
    }

    /// And when the wording carries no name at all, it says so rather than inventing one. A
    /// refusal naming the wrong tool sends somebody to widen a permission that was never the
    /// problem.
    #[test]
    fn a_refusal_with_no_tool_in_it_does_not_invent_one() {
        let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"x","is_error":true,"content":"you have not granted permission for that"}]}}"#;
        match chunk_of(line) {
            Some(Chunk::Refused { tool, .. }) => assert_eq!(tool, "a tool"),
            other => panic!("not recognised as a refusal: {other:?}"),
        }
    }

    /// The agent tools are absent by decision. Sending somebody to permissions.json would
    /// invite them to undo that, so the refusal names the route that does exist instead.
    #[test]
    fn a_refused_agent_tool_points_at_the_handoff_rather_than_permissions() {
        for tool in ["Agent", "Task", "ListAgents", "SendMessage"] {
            let line = crate::claude::refusal_line(tool, "not granted");
            assert!(line.contains(tool), "it still names the tool: {line}");
            assert!(line.contains("carl handoff"), "it names the route: {line}");
            assert!(
                !line.contains("Add"),
                "it must not tell anybody to widen a permission: {line}"
            );
        }

        let search = crate::claude::refusal_line("ToolSearch", "not granted");
        assert!(search.contains("no wider set to search"), "{search}");
        assert!(!search.contains("Add"), "{search}");
    }

    #[test]
    fn the_refusal_names_what_to_add_and_where() {
        let line = crate::claude::refusal_line("Bash(python3:*)", "not granted");
        assert!(line.contains("Bash(python3:*)"), "{line}");
        assert!(line.contains("permissions.json"), "{line}");
        assert!(
            line.contains("headless"),
            "it has to say why nobody can approve: {line}"
        );
    }

    /// JJ asked to see what Carl is doing while he thinks.
    ///
    /// Between the question and the first word there was only a caret, so a Carl reading forty
    /// files looked exactly like a Carl that had wedged. He already says which tool he picked
    /// up on every assistant line, and it was being thrown away.
    #[test]
    fn a_tool_carl_picks_up_is_carried_out_of_the_stream() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"cargo test --lib"}}]}}"#;
        match chunk_of(line) {
            Some(Chunk::Doing { tool, detail }) => {
                assert_eq!(tool, "Bash");
                assert_eq!(detail, "cargo test --lib");
            }
            other => panic!("expected a Doing, got {other:?}"),
        }
    }

    /// Each tool names its interesting field differently, and a note with no detail is useless.
    #[test]
    fn the_detail_is_taken_from_whichever_field_the_tool_uses() {
        let read = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read","input":{"file_path":"/tmp/x.rs"}}]}}"#;
        match chunk_of(read) {
            Some(Chunk::Doing { tool, detail }) => {
                assert_eq!(tool, "Read");
                assert_eq!(detail, "/tmp/x.rs");
            }
            other => panic!("expected a Doing, got {other:?}"),
        }

        let grep = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Grep","input":{"pattern":"fn main"}}]}}"#;
        match chunk_of(grep) {
            Some(Chunk::Doing { detail, .. }) => assert_eq!(detail, "fn main"),
            other => panic!("expected a Doing, got {other:?}"),
        }
    }

    /// An assistant line with no tool in it is still not spoken again.
    #[test]
    fn an_assistant_line_of_plain_text_carries_nothing() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"the whole answer so far"}]}}"#;
        assert!(
            chunk_of(line).is_none(),
            "the repeated answer was carried out as a chunk"
        );
    }

    /// A long command must not push the answer off the screen.
    #[test]
    fn a_long_detail_is_cut_rather_than_wrapped() {
        let long = "a".repeat(300);
        let line = crate::claude::doing_line("Bash", &long);
        assert!(line.len() < 120, "not cut: {} chars", line.len());
        assert!(line.contains("..."), "and it does not say it was cut");
    }

    /// A tool with nothing worth reading still says which tool.
    #[test]
    fn a_call_with_no_detail_still_names_the_tool() {
        let line = crate::claude::doing_line("Glob", "");
        assert!(line.contains("Glob"), "{line:?}");
    }
}
