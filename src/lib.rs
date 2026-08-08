//! Carl, a general purpose helper that remembers.
//!
//! Three layers, and keeping them apart is the whole design.
//!
//! | layer | what it is | how long it lasts |
//! |---|---|---|
//! | the log | every message ever, appended | forever |
//! | the thread | one Claude Code session, resumed | until it is compacted |
//! | memory | notes Carl chose to keep | forever, and small |
//!
//! The log is the record. It never shrinks and nothing is ever deleted from it. It is not
//! what gets sent to the model.
//!
//! The thread is what gets sent. It is bounded, because a context window is finite, and
//! Claude Code compacts it when it grows too long. Losing detail here is fine, because the
//! log still has it.
//!
//! Memory is what survives moving between conversations. Resuming a thread gives back and
//! forth inside that thread. It does not make Carl remember something said in a different
//! Slack thread last week. Only memory does that.

pub mod claude;
pub mod log;
pub mod memory;
pub mod threads;

pub use log::{Entry, Log, Speaker};
pub use memory::Memory;
pub use threads::{Registry, SessionId};

use serde::{Deserialize, Serialize};

/// Names one conversation. A Slack thread, or `cli` when talking to Carl from a terminal.
///
/// Validated once here, because a thread id becomes part of a filename. Anything that could
/// mean "parent directory" or "separator" to a path is refused at construction rather than
/// guarded at every use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        let ok = !raw.is_empty()
            && raw.len() <= 128
            && raw
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_')
            && !raw.starts_with('.')
            && raw != ".."
            && !raw.contains("..");

        if ok {
            Ok(Self(raw))
        } else {
            Err(Error::BadThreadId(raw))
        }
    }

    /// A Slack thread, named so it stays readable in the log.
    ///
    /// Slack timestamps contain a dot, which is why the character set allows one. A leading
    /// dot and any `..` are still refused.
    pub fn slack(channel: &str, thread_ts: &str) -> Result<Self> {
        Self::new(format!("slack-{channel}-{thread_ts}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ThreadId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<ThreadId> for String {
    fn from(value: ThreadId) -> Self {
        value.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("thread id must be letters, digits, dash, dot or underscore, got {0:?}")]
    BadThreadId(String),

    #[error("refused: {0}")]
    Refused(String),

    #[error("claude failed: {0}")]
    Claude(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_accepted() {
        assert_eq!(ThreadId::new("cli").unwrap().as_str(), "cli");
        assert_eq!(
            ThreadId::slack("C01ABC", "1700000000.123456")
                .unwrap()
                .as_str(),
            "slack-C01ABC-1700000000.123456"
        );
    }

    /// A thread id becomes part of a filename, so every path shaped form is refused here
    /// rather than at each use site.
    #[test]
    fn path_shaped_names_are_refused() {
        for bad in [
            "",
            "..",
            "../etc",
            "a/b",
            "a\\b",
            ".hidden",
            "a b",
            "a:b",
            "nul\0byte",
            "a..b",
        ] {
            assert!(ThreadId::new(bad).is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn overlong_names_are_refused() {
        assert!(ThreadId::new("a".repeat(128)).is_ok());
        assert!(ThreadId::new("a".repeat(129)).is_err());
    }

    /// Deserialising runs the same check, so a hand edited registry cannot smuggle one in.
    #[test]
    fn deserialising_a_bad_id_fails() {
        assert!(serde_json::from_str::<ThreadId>("\"../../etc/passwd\"").is_err());
    }
}
