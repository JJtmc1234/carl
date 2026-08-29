//! Whether an agent's memory is in a state it can actually work from.
//!
//! Derived every time it is asked, from the files themselves. There is no health file and there
//! must not be one: a stored verdict is a second source of truth that goes stale silently, and
//! the failure it would hide is exactly the one worth catching.
//!
//! The failure this exists for is an agent that looks fine. A process can be up, a session can
//! be resumed, and the agent can still have lost the folder it keeps everything in. `carl army
//! who` reported that agent as idle, which is true and useless. A healthy process running a
//! memory broken agent is worse than a stopped one, because nothing draws attention to it.

use std::path::Path;

use super::personnel::{Learned, memory};

/// What is wrong with an agent's memory, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Memory {
    /// Everything the layout expects is present and readable.
    Fine,
    /// No memory folder at all. The agent has never been seeded, or it was removed.
    NoFolder,
    /// The folder is there and the file the agent is told to read first is not.
    NoSummary,
    /// Made before the current layout and never brought up to it. `carl army migrate` fixes it.
    Unmigrated(&'static str),
    /// Present but not readable as what it claims to be.
    Malformed(String),
}

impl Memory {
    /// Whether this is worth interrupting somebody about.
    pub fn is_a_problem(&self) -> bool {
        !matches!(self, Self::Fine)
    }

    /// One word for a column, and a longer line lives in `why`.
    pub fn word(&self) -> &'static str {
        match self {
            Self::Fine => "ok",
            Self::NoFolder => "no folder",
            Self::NoSummary => "no summary",
            Self::Unmigrated(_) => "unmigrated",
            Self::Malformed(_) => "malformed",
        }
    }

    /// What to tell somebody, including what to do about it.
    pub fn why(&self) -> String {
        match self {
            Self::Fine => "memory is complete".into(),
            Self::NoFolder => {
                "no memory folder. This agent has nothing to keep anything in, so every session \
                 starts from nothing. Run `carl army migrate`"
                    .into()
            }
            Self::NoSummary => {
                "no summary.md. It is the one file the agent is told to read first, so the \
                 folder is there and nothing points into it. Run `carl army migrate`"
                    .into()
            }
            Self::Unmigrated(what) => {
                format!("{what}. Run `carl army migrate`, which only adds what is missing")
            }
            Self::Malformed(why) => why.clone(),
        }
    }
}

/// What is actually on disk for one agent, and the verdict that follows from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    pub memory: Memory,
    /// Promoted rules in `learned.md`. `None` when the file could not be read.
    pub rules: Option<usize>,
    /// Patterns counted but not yet promoted.
    pub watching: Option<usize>,
    pub summary_bytes: u64,
    /// The hand written file the current layout migrated away from.
    pub legacy_rules: bool,
}

/// Reads one agent's memory folder and says what state it is in.
///
/// Never writes. Never repairs. Something that quietly fixed what it was asked to report on
/// would make the report a description of its own side effect.
pub fn of(folder: &Path) -> Health {
    let dir = memory::dir(folder);
    let summary = memory::summary_path(folder);
    let learned_at = memory::learned_path(folder);
    let legacy = dir.join(memory::LEGACY_RULES);

    let summary_bytes = summary.metadata().map(|m| m.len()).unwrap_or(0);
    let legacy_rules = legacy.is_file();

    let (rules, watching, learned_broken) = match Learned::load(&learned_at) {
        Ok(l) => (Some(l.rules().len()), Some(l.watching().len()), None),
        Err(e) => (None, None, Some(format!("learned.md is unreadable: {e}"))),
    };

    let memory = if !dir.is_dir() {
        Memory::NoFolder
    } else if let Some(why) = learned_broken {
        Memory::Malformed(why)
    } else if !summary.is_file() {
        Memory::NoSummary
    } else if !learned_at.is_file() {
        Memory::Unmigrated("no learned.md, so nothing this agent works out can be kept")
    } else if legacy_rules && rules == Some(0) {
        // The old file still holds the only standing decisions there are, and nothing reads it
        // any more, so this agent has quietly lost them.
        Memory::Unmigrated("rules.md still holds the standing decisions and nothing reads it")
    } else {
        Memory::Fine
    };

    Health {
        memory,
        rules,
        watching,
        summary_bytes,
        legacy_rules,
    }
}

#[cfg(test)]
mod tests;
