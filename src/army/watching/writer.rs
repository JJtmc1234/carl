//! The half of `watching` that a caller holds: one turn of one agent, written as it happens.

use std::path::{Path, PathBuf};

use super::file::append;
use super::{Line, MOST_DETAIL, Note, TOKEN_STEP, path};
use crate::claude::Say;

/// Writes down what one agent is doing, for the length of one turn.
///
/// **Nothing here can fail a turn.** Every write swallows its error. An agent that did a piece
/// of work and could not tell anybody about it has still done the work, and taking the turn
/// down because a log file was unwritable would trade the thing that matters for the thing that
/// describes it.
pub struct Watching {
    path: PathBuf,
    agent: String,
    /// The token count of the last thinking note written this turn, and `None` when none has
    /// been. Reasoning is recorded when it has grown rather than on every delta, but the first
    /// one always goes down: a CLI that gives no sizes at all would otherwise record no
    /// reasoning ever, and "thinking is happening" is itself worth one row.
    wrote_at_tokens: Option<u32>,
}

impl Watching {
    pub fn of(home: &Path, agent: &str) -> Self {
        Self {
            path: path(home),
            agent: agent.to_string(),
            wrote_at_tokens: None,
        }
    }

    /// A question going in. Also resets the reasoning counter, since a new turn counts again.
    pub fn asked(&mut self, prompt: &str) {
        self.wrote_at_tokens = None;
        self.write(Note::Asked {
            chars: prompt.chars().count(),
        });
    }

    /// One piece of the stream. Call this from the `on_text` closure and nothing else changes.
    pub fn saw(&mut self, say: Say<'_>) {
        match say {
            // Not recorded. The answer goes to whoever asked and into the transcript, and a
            // second copy here would be a second place for it to disagree with itself.
            Say::Words(_) => {}
            Say::Thinking { text, tokens } => {
                let grown = match (self.wrote_at_tokens, tokens) {
                    (None, _) => true,
                    (Some(last), Some(now)) => now >= last + TOKEN_STEP,
                    (Some(_), None) => false,
                };
                // Real reasoning text is never dropped, however often it arrives, because on
                // the day it stops being redacted it is the whole point of this file.
                if !text.is_empty() || grown {
                    self.wrote_at_tokens = Some(tokens.unwrap_or(0));
                    self.write(Note::Thinking {
                        text: text.to_string(),
                        tokens,
                    });
                }
            }
            Say::Doing { tool, detail } => self.write(Note::Doing {
                tool: tool.to_string(),
                detail: shorten(detail),
            }),
            Say::Refused { tool, why } => self.write(Note::Refused {
                tool: tool.to_string(),
                why: why.to_string(),
            }),
        }
    }

    /// The turn ending, however it ended.
    pub fn answered(&mut self, said: &str, interrupted: bool) {
        self.write(Note::Answered {
            chars: said.chars().count(),
            interrupted,
        });
    }

    fn write(&self, note: Note) {
        let line = Line {
            at: crate::army::event::now(),
            agent: self.agent.clone(),
            note,
        };
        let _ = append(&self.path, &line);
    }
}

/// The first `MOST_DETAIL` characters, on a character boundary, flattened onto one line.
///
/// One line because the file is one JSON object per line and a reader shows one note per row,
/// so an embedded newline would make a tool call look like two things happening.
fn shorten(detail: &str) -> String {
    let flat = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(MOST_DETAIL) {
        Some((at, _)) => format!("{}...", &flat[..at]),
        None => flat,
    }
}
