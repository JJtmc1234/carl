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

/// The fixed header every block starts with, held as a constant so its cost can be reserved
/// rather than added on afterwards.
const PREAMBLE: &str = "# What you remember\n\n\
     These are notes you kept from earlier conversations. They may be out of date. \
     If one turns out to be wrong, drop it with a [forget] line naming its heading, rather than acting on it or carrying on around it.\n\n";

/// How many skipped notes to name before summarising the rest.
///
/// Naming all of them is what made the shortfall note unbounded. A handful of names is enough
/// to be useful, and the count carries the rest of the meaning.
const SKIPPED_NAMED: usize = 5;

/// The note about what did not fit, bounded whatever the pile looks like.
///
/// Empty when nothing was skipped. Naming every skipped note is what made this unbounded, so it
/// names a handful and counts the rest, which keeps its length within a few hundred bytes
/// however many thousand notes there are.
fn shortfall(skipped: &[String]) -> String {
    if skipped.is_empty() {
        return String::new();
    }

    let named: Vec<&str> = skipped
        .iter()
        .take(SKIPPED_NAMED)
        .map(String::as_str)
        .collect();
    let rest = skipped.len() - named.len();
    let tail = if rest > 0 {
        format!(", and {rest} more")
    } else {
        String::new()
    };

    let full = format!(
        "## note\n\nThese memories were left out because the budget is full: {}{tail}. \
         Say so if one of them seems relevant.\n\n",
        named.join(", ")
    );
    full
}

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

        // The budget has to cover the whole block, and both the preamble and the note about
        // what did not fit used to sit outside the check entirely. Worse, that note grew by one
        // filename for every note that was skipped, so the overshoot grew with the size of the
        // pile: the one thing the budget exists to bound was the one thing unbounded. A few
        // thousand notes put it over a hundred kilobytes, on every message. See bug 26.
        let room = self.budget.saturating_sub(PREAMBLE.len());

        let mut kept: Vec<(String, String)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        let mut used = 0usize;

        for path in &notes {
            let body = std::fs::read_to_string(path)?;
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let piece = format!("## {name}\n\n{}\n\n", body.trim());

            if used + piece.len() > room {
                skipped.push(name.into_owned());
                continue;
            }
            used += piece.len();
            kept.push((name.into_owned(), piece));
        }

        // Then pay for the note about what was skipped, by giving notes back until the finished
        // block fits. Worked out rather than guessed at, because it feeds itself: giving a note
        // back adds its name to the note, which makes the note longer. A fixed reservation
        // would either be too small to be safe or large enough to swallow a modest budget
        // whole, and this was tried both ways round before it was written like this.
        let note = loop {
            let note = shortfall(&skipped);
            if PREAMBLE.len() + used + note.len() <= self.budget {
                break note;
            }
            let Some((name, piece)) = kept.pop() else {
                break note;
            };
            used -= piece.len();
            skipped.push(name);
        };

        if kept.is_empty() {
            return Ok(None);
        }

        let mut out = String::with_capacity(used + note.len());
        for (_, piece) in &kept {
            out.push_str(piece);
        }
        // Say what was dropped rather than truncating in silence. A memory that quietly
        // stopped being read is worse than one that is obviously missing.
        out.push_str(&note);

        Ok(Some(format!("{PREAMBLE}{out}")))
    }

    /// Writes or replaces one note.
    pub fn write(&self, name: &str, body: &str) -> Result<PathBuf> {
        let path = self.path_for(name)?;
        std::fs::write(&path, format!("{}\n", body.trim()))?;
        Ok(path)
    }

    /// Writes a note and records who said it.
    ///
    /// Memory is one pile and everybody who can reach Carl writes into it. Without a source,
    /// something Hunter mentioned about his own base comes back later as a fact about JJ's,
    /// stated with the same confidence as anything JJ said himself.
    ///
    /// Kept as a line in the note rather than in the filename or a separate index. The note is
    /// handed to a model as text, so the attribution has to survive as text, and a second file
    /// to keep in step is a second file to get out of step.
    pub fn write_from(&self, name: &str, body: &str, source: &str) -> Result<PathBuf> {
        let source = source.trim();
        if source.is_empty() {
            return self.write(name, body);
        }
        self.write(name, &format!("{}\n\n(said by {source})", body.trim()))
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

    /// Memory is one pile and everybody who can reach Carl writes into it. Without a source,
    /// something Hunter said about his own base comes back later as a fact about JJ's.
    #[test]
    fn a_note_records_who_said_it() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        let p = m
            .write_from("blue-science", "The base is on blue science", "Hunter")
            .unwrap();
        let body = std::fs::read_to_string(&p).unwrap();

        assert!(body.contains("The base is on blue science"));
        assert!(body.contains("said by Hunter"), "{body}");
    }

    /// It has to survive being read back and handed to a model, because that is the only
    /// place it is ever used.
    #[test]
    fn the_source_comes_back_in_the_assembled_memory() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();
        m.write_from("a-fact", "Something", "Hunter").unwrap();

        let assembled = m.assemble().unwrap().unwrap();
        assert!(assembled.contains("said by Hunter"), "{assembled}");
    }

    /// No source is not the string "said by nobody". A note from an unknown speaker is still
    /// a note.
    #[test]
    fn an_unknown_speaker_leaves_the_note_alone() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        for source in ["", "   "] {
            let p = m.write_from("plain", "Just a fact", source).unwrap();
            let body = std::fs::read_to_string(&p).unwrap();
            assert_eq!(body.trim(), "Just a fact", "{body}");
        }
    }

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

    /// The bug. The shortfall note listed every skipped filename and was appended outside the
    /// budget check, so the overshoot grew with the number of notes that did not fit. The one
    /// thing the budget exists to bound was the one thing unbounded, and it rides on every
    /// single message.
    #[test]
    fn a_large_pile_still_fits_the_budget() {
        let (m, _d) = memory();
        let m = m.with_budget(DEFAULT_BUDGET);

        // Enough that almost all of them are skipped, which is what used to make the note
        // enormous. Each is small, so the pile fails on count rather than on size.
        for i in 0..3_000 {
            m.write(&format!("note-{i:05}"), "a small remembered fact")
                .unwrap();
        }

        let text = m.assemble().unwrap().unwrap();
        assert!(
            text.len() <= DEFAULT_BUDGET,
            "{} bytes against a {DEFAULT_BUDGET} byte budget",
            text.len()
        );
        assert!(
            text.contains("more"),
            "the count of what was left out still has to be said: {}",
            &text[text.len().saturating_sub(200)..]
        );
    }

    /// A budget too small to hold the preamble and one note yields nothing rather than
    /// something over budget. Silence is the honest answer there.
    #[test]
    fn a_budget_too_small_for_anything_injects_nothing() {
        let (m, _d) = memory();
        let m = m.with_budget(50);
        m.write("a", "something").unwrap();
        assert!(m.assemble().unwrap().is_none());
    }

    /// The whole assembled block counts, at every budget, which is the property the old test
    /// did not check and the fix exists to hold.
    #[test]
    fn the_whole_block_fits_whatever_the_budget() {
        for budget in [300usize, 600, 1_000, 5_000, 24_000] {
            let (m, _d) = memory();
            let m = m.with_budget(budget);
            for i in 0..200 {
                m.write(&format!("n{i:03}"), &"x".repeat(120)).unwrap();
            }
            if let Some(text) = m.assemble().unwrap() {
                assert!(
                    text.len() <= budget,
                    "{} bytes against a {budget} byte budget",
                    text.len()
                );
            }
        }
    }

    /// Memory rides on every message, so an unbounded pile is a bill that grows quietly.
    #[test]
    fn the_budget_is_enforced_and_the_shortfall_is_named() {
        let (m, _d) = memory();
        let m = m.with_budget(400);

        m.write("aaa-small", "short note").unwrap();
        m.write("zzz-huge", &"x".repeat(2000)).unwrap();

        let text = m.assemble().unwrap().unwrap();
        // Against the budget itself, not against three times it. The old assertion allowed
        // 1200 bytes for a 400 byte budget, which is why the overshoot went unnoticed.
        assert!(
            text.len() <= 400,
            "budget ignored, got {} bytes for a 400 byte budget",
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
