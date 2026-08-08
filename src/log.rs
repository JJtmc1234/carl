//! The permanent record.
//!
//! Every message in every conversation, appended and never rewritten. This is the layer that
//! genuinely is forever. It is not what gets sent to the model, and keeping those two ideas
//! apart is the whole design. See the note in `readme.md`.
//!
//! One JSON object per line, so the record stays readable with `cat` when whatever wrote it
//! is the thing that is broken.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, ThreadId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Speaker {
    /// A person, whether in Slack or at a terminal.
    Human,
    /// Carl.
    Carl,
    /// Something the runtime recorded that nobody said, like an error.
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    /// Unix seconds. Passed in rather than read from the clock, so entries are testable.
    pub at: u64,
    pub thread: ThreadId,
    pub speaker: Speaker,
    pub text: String,
    /// Where the message came from, for example a Slack user id. Absent for local use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

/// Append only. There is deliberately no delete and no edit.
pub struct Log {
    file: std::fs::File,
    next_seq: u64,
}

impl Log {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let next_seq = read(path)?.last().map_or(1, |e| e.seq + 1);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        Ok(Self { file, next_seq })
    }

    pub fn append(
        &mut self,
        at: u64,
        thread: ThreadId,
        speaker: Speaker,
        text: impl Into<String>,
        author: Option<String>,
    ) -> Result<Entry> {
        let entry = Entry {
            seq: self.next_seq,
            at,
            thread,
            speaker,
            text: text.into(),
            author,
        };
        // One write call, so two processes appending cannot interleave a partial line.
        self.file
            .write_all(format!("{}\n", serde_json::to_string(&entry)?).as_bytes())?;
        self.file.flush()?;
        self.next_seq += 1;
        Ok(entry)
    }
}

/// Every entry, in order. A missing file is an empty log rather than an error, because the
/// first run has nothing to read.
pub fn read(path: impl AsRef<Path>) -> Result<Vec<Entry>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Ok(serde_json::from_str(line)?))
        .collect()
}

/// Just one conversation, in order.
pub fn thread(entries: &[Entry], thread: &ThreadId) -> Vec<Entry> {
    entries
        .iter()
        .filter(|e| &e.thread == thread)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str) -> ThreadId {
        ThreadId::new(name).unwrap()
    }

    #[test]
    fn entries_survive_reopening_and_keep_counting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.jsonl");

        let mut log = Log::open(&path).unwrap();
        assert_eq!(
            log.append(1, t("a"), Speaker::Human, "hi", None)
                .unwrap()
                .seq,
            1
        );
        assert_eq!(
            log.append(2, t("a"), Speaker::Carl, "hello", None)
                .unwrap()
                .seq,
            2
        );

        // Reopening must not truncate and must not reuse a sequence number. A record that
        // loses history on restart is worse than no record, because it looks complete.
        let mut reopened = Log::open(&path).unwrap();
        assert_eq!(
            reopened
                .append(3, t("b"), Speaker::Human, "hey", None)
                .unwrap()
                .seq,
            3
        );
        assert_eq!(read(&path).unwrap().len(), 3);
    }

    #[test]
    fn a_missing_log_reads_as_empty() {
        assert!(read("/tmp/carl-no-such-log.jsonl").unwrap().is_empty());
    }

    #[test]
    fn threads_are_kept_apart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.jsonl");
        let mut log = Log::open(&path).unwrap();

        log.append(1, t("slack-1"), Speaker::Human, "about the roof", None)
            .unwrap();
        log.append(2, t("slack-2"), Speaker::Human, "about the car", None)
            .unwrap();
        log.append(3, t("slack-1"), Speaker::Carl, "which roof", None)
            .unwrap();

        let all = read(&path).unwrap();
        let one = thread(&all, &t("slack-1"));
        assert_eq!(one.len(), 2);
        assert!(one.iter().all(|e| e.thread == t("slack-1")));
        assert_eq!(thread(&all, &t("slack-2")).len(), 1);
    }

    #[test]
    fn a_message_with_a_newline_stays_one_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversations.jsonl");
        let mut log = Log::open(&path).unwrap();

        // JSON escapes the newline, so a multi line message cannot become two records.
        log.append(1, t("a"), Speaker::Human, "line one\nline two", None)
            .unwrap();

        let all = read(&path).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].text, "line one\nline two");
    }
}
