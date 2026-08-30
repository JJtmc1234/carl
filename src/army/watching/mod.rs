//! What an agent is doing while it works, written down as it happens.
//!
//! The army has been opaque in the one way that matters. `carl army status` says a process is
//! up, `carl army activity` says work was handed down, and between those two facts an agent can
//! spend four minutes doing something nobody will ever see. When JJ asked what the agents were
//! thinking there was no answer, because three of the four places that hold an agent's pipe
//! threw the stream away: `chain::run` passed `&mut |_| Flow::Continue`, and so did
//! `Supervisor::deliver`. Only the panel, and only when JJ himself was the one asking, ever
//! looked at a `Say` that was not words.
//!
//! **The reasoning text is redacted at the source and this cannot fix that.** The CLI emits
//! `{"type":"thinking_delta","thinking":"","estimated_tokens":50}`, so the words are gone before
//! any of our code sees them. What is left is real and is worth having: how much reasoning is
//! happening, which tool was picked up and with what, and what was refused for want of
//! permission. A tool call is not a paraphrase of a thought, it is the thought acted on, and
//! reading forty of them in order is how you can tell an agent reading its memory from an agent
//! stuck in a loop. `text` is stored anyway, empty in practice, so that the day the CLI stops
//! redacting there is nothing here to change.
//!
//! Its own file rather than the journal. The journal is the sequenced record of decisions and a
//! turn produces hundreds of these, so putting them together would drown the thing the journal
//! exists to make countable. This file is bounded and disposable on purpose: losing it loses
//! nothing that was a decision.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

mod file;
mod read;
mod writer;

pub use read::{line_of, read, since};
pub use writer::Watching;

/// Where the notes live. One file for the whole army, because the interesting question is
/// usually "what is happening", and a file per agent makes that question a directory scan.
pub fn path(home: &Path) -> PathBuf {
    home.join("run").join("working.jsonl")
}

/// How much of a tool's input is kept.
///
/// A `Write` call carries the whole file. Keeping it would make this file mostly one tool call,
/// and the first line of the input is what says which file and roughly why.
const MOST_DETAIL: usize = 200;

/// How big the file may get before the older half is dropped.
const MOST_BYTES: u64 = 2 * 1024 * 1024;

/// How much the reasoning has to grow before another note is worth writing.
///
/// Thinking deltas arrive every few tokens, so one note each would be thousands of lines per
/// turn saying the same thing slightly larger. The count is what carries the information, so
/// the note is worth writing when the count has actually moved.
const TOKEN_STEP: u32 = 100;

/// One thing that happened while an agent was working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Note {
    /// A question went in. The words are not kept: they are already in whatever asked.
    Asked { chars: usize },
    /// Reasoning. `text` is empty in practice, see the note at the top of this file.
    Thinking {
        #[serde(default)]
        text: String,
        tokens: Option<u32>,
    },
    /// A tool picked up, with as much of its input as is worth keeping.
    Doing { tool: String, detail: String },
    /// A tool call refused for want of permission. The one kind here that is addressed to a
    /// person, because the person reading it is the one who can widen the list.
    Refused { tool: String, why: String },
    /// The turn ended. Interrupted means the deadline ran out, not that the agent stopped.
    Answered { chars: usize, interrupted: bool },
}

/// One line of the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub at: u64,
    pub agent: String,
    #[serde(flatten)]
    pub note: Note,
}

#[cfg(test)]
mod tests;
