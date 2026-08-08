//! What Carl remembers across different conversations.
//!
//! Resuming a thread replays that thread. It does nothing for a question asked in a different
//! Slack thread last week, and that is the gap people mean when they say an assistant should
//! remember them. Memory closes it.
//!
//! Markdown files rather than a database, for two reasons. A person can read and correct
//! them, which matters when Carl remembers something wrong. And they go into the model the
//! same way any other instruction does, with no retrieval layer to go wrong.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// A directory of notes, injected into every conversation.
pub struct Memory {
    dir: PathBuf,
    /// Refuse to inject past this many bytes. Memory rides along on every single message, so
    /// an unbounded pile is a bill that grows quietly and forever.
    budget: usize,
}

/// Roughly four thousand words. Large enough to be useful, small enough to notice.
pub const DEFAULT_BUDGET: usize = 24_000;

impl Memory {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            budget: DEFAULT_BUDGET,
        })
    }

    pub fn with_budget(mut self, bytes: usize) -> Self {
        self.budget = bytes;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every note, oldest name first, as one block of markdown.
    ///
    /// Returns `None` when there is nothing to say. An empty section in the prompt invites
    /// the model to fill it, so it is better to say nothing at all.
    pub fn assemble(&self) -> Result<Option<String>> {
        let mut notes = self.notes()?;
        notes.sort();

        let mut out = String::new();
        let mut skipped = Vec::new();

        for path in &notes {
            let body = std::fs::read_to_string(path)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let piece = format!("## {name}\n\n{}\n\n", body.trim());

            if out.len() + piece.len() > self.budget {
                skipped.push(name.into_owned());
                continue;
            }
            out.push_str(&piece);
        }

        if out.is_empty() {
            return Ok(None);
        }

        // Say what was dropped rather than truncating in silence. A memory that quietly
        // stopped being read is worse than one that is obviously missing.
        if !skipped.is_empty() {
            out.push_str(&format!(
                "## note\n\nThese memories were left out because the budget is full: {}. \
                 Say so if one of them seems relevant.\n\n",
                skipped.join(", ")
            ));
        }

        Ok(Some(format!(
            "# What you remember\n\n\
             These are notes you kept from earlier conversations. They may be out of date. \
             If one turns out to be wrong, say so rather than acting on it.\n\n{out}"
        )))
    }

    /// Writes or replaces one note.
    pub fn write(&self, name: &str, body: &str) -> Result<PathBuf> {
        let path = self.path_for(name)?;
        std::fs::write(&path, format!("{}\n", body.trim()))?;
        Ok(path)
    }

    pub fn forget(&self, name: &str) -> Result<bool> {
        let path = self.path_for(name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub fn notes(&self) -> Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "md") {
                out.push(path);
            }
        }
        Ok(out)
    }

    /// Note names are filenames, so they get the same treatment as thread ids.
    fn path_for(&self, name: &str) -> Result<PathBuf> {
        let ok = !name.is_empty()
            && name.len() <= 64
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            && !name.starts_with('-');

        if !ok {
            return Err(Error::Refused(format!(
                "memory name must be letters, digits, dash or underscore, got {name:?}"
            )));
        }
        Ok(self.dir.join(format!("{name}.md")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> (Memory, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let m = Memory::open(dir.path().join("memory")).unwrap();
        (m, dir)
    }

    #[test]
    fn nothing_remembered_means_nothing_injected() {
        let (m, _d) = memory();
        assert!(
            m.assemble().unwrap().is_none(),
            "an empty section invites invention"
        );
    }

    #[test]
    fn notes_come_back_in_the_assembled_block() {
        let (m, _d) = memory();
        m.write("jj", "JJ is 11 and writes Rust.").unwrap();
        m.write("style", "No dashes. No semicolons.").unwrap();

        let text = m.assemble().unwrap().unwrap();
        assert!(text.contains("JJ is 11"), "{text}");
        assert!(text.contains("No dashes"), "{text}");
        // The framing matters as much as the content.
        assert!(text.contains("may be out of date"), "{text}");
    }

    #[test]
    fn forgetting_removes_it() {
        let (m, _d) = memory();
        m.write("wrong", "JJ is 40.").unwrap();
        assert!(m.assemble().unwrap().unwrap().contains("JJ is 40"));

        assert!(m.forget("wrong").unwrap());
        assert!(m.assemble().unwrap().is_none());
        assert!(
            !m.forget("wrong").unwrap(),
            "forgetting twice is not an error"
        );
    }

    /// Memory rides on every message, so an unbounded pile is a bill that grows quietly.
    #[test]
    fn the_budget_is_enforced_and_the_shortfall_is_named() {
        let (m, _d) = memory();
        let m = m.with_budget(400);

        m.write("aaa-small", "short note").unwrap();
        m.write("zzz-huge", &"x".repeat(2000)).unwrap();

        let text = m.assemble().unwrap().unwrap();
        assert!(
            text.len() < 1200,
            "budget ignored, got {} bytes",
            text.len()
        );
        assert!(text.contains("short note"), "the small note should survive");
        assert!(
            text.contains("zzz-huge.md"),
            "a dropped memory must be named rather than vanish: {text}"
        );
    }

    #[test]
    fn note_names_cannot_escape_the_directory() {
        let (m, _d) = memory();
        for bad in ["../escape", "a/b", "", "-leading", "a b", "a.b"] {
            assert!(m.write(bad, "x").is_err(), "{bad:?} should be refused");
        }
    }

    #[test]
    fn non_markdown_files_are_ignored() {
        let (m, _d) = memory();
        m.write("real", "a real note").unwrap();
        std::fs::write(m.dir().join("notes.txt"), "not markdown").unwrap();
        std::fs::write(m.dir().join("scratch.json"), "{}").unwrap();

        let text = m.assemble().unwrap().unwrap();
        assert!(text.contains("a real note"));
        assert!(!text.contains("not markdown"), "{text}");
    }
}
