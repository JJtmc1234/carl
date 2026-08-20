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

/// The longest a note's filename may be, before `.md`.
///
/// `remember::note_name` keeps well under this deliberately, so a name it produces is always
/// writable. It used to cap the number of words and never the number of bytes, so six long
/// words made a name this refused and the note was lost. See bug 20.
pub const NAME_BYTES: usize = 64;

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
             If one turns out to be wrong, drop it with a [forget] line naming its heading, rather than acting on it or carrying on around it.\n\n{out}"
        )))
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
        // Length said separately from shape. A slug made of letters and dashes that was simply
        // too long used to be refused with a message blaming the character set, which sends
        // whoever reads it looking in the wrong place entirely. See bug 20.
        if name.len() > NAME_BYTES {
            return Err(Error::Refused(format!(
                "a memory name may be {NAME_BYTES} bytes and this one is {}: {name:?}",
                name.len()
            )));
        }

        let ok = !name.is_empty()
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
    /// Two facts opening with the same six words both survive. The whole of bug 19.
    ///
    /// Against the real store rather than against the naming function, because the damage was
    /// a file being overwritten and its attribution going with it, which a name comparison on
    /// its own does not show.
    #[test]
    fn two_facts_that_start_the_same_do_not_overwrite_each_other() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        let north = "The red belt line at the north outpost is jammed";
        let south = "The red belt line at the south outpost is fine";
        m.write_from(&crate::remember::note_name(north), north, "JJ")
            .unwrap();
        m.write_from(&crate::remember::note_name(south), south, "Hunter")
            .unwrap();

        let all = m.assemble().unwrap().expect("both notes should be there");
        assert!(all.contains("north outpost is jammed"), "{all}");
        assert!(all.contains("south outpost is fine"), "{all}");
        assert!(
            all.contains("said by JJ"),
            "attribution went with it: {all}"
        );
        assert!(all.contains("said by Hunter"), "{all}");
    }

    /// And forgetting one of them leaves the other, since both go through the same name.
    #[test]
    fn forgetting_one_of_two_similar_facts_leaves_the_other() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        let north = "The red belt line at the north outpost is jammed";
        let south = "The red belt line at the south outpost is fine";
        for (note, who) in [(north, "JJ"), (south, "Hunter")] {
            m.write_from(&crate::remember::note_name(note), note, who)
                .unwrap();
        }

        assert!(m.forget(&crate::remember::note_name(north)).unwrap());
        let left = m.assemble().unwrap().expect("one note should be left");
        assert!(!left.contains("north outpost"), "{left}");
        assert!(left.contains("south outpost is fine"), "{left}");
    }

    /// A note with six long words has to reach the disk. The whole of bug 20.
    ///
    /// `note_name` capped the number of words and never the number of bytes, so this returned
    /// a 76 byte slug, `path_for` refused it, and the note was lost while Carl had already
    /// said in the same breath that he had kept it.
    #[test]
    fn a_note_with_long_words_is_still_written() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        let note = "Hunter recommended reconfiguring authentication credentials \
                    programmatically throughout deployment";
        let name = crate::remember::note_name(note);
        m.write_from(&name, note, "Hunter")
            .expect("a note Carl said he kept must actually be kept");

        let all = m.assemble().unwrap().expect("the note should be there");
        assert!(all.contains("reconfiguring authentication"), "{all}");
    }

    /// A refusal has to name the reason it actually refused for.
    ///
    /// The old message blamed the character set for a name made of letters and dashes that was
    /// simply too long, which sends whoever reads it looking in the wrong place.
    #[test]
    fn a_name_that_is_too_long_says_so_rather_than_blaming_the_letters() {
        let d = tempfile::tempdir().unwrap();
        let m = Memory::open(d.path()).unwrap();

        let name = "a".repeat(NAME_BYTES + 1);
        let why = m.write(&name, "anything").unwrap_err().to_string();

        assert!(why.contains(&format!("{NAME_BYTES} bytes")), "{why}");
        assert!(
            !why.contains("letters, digits"),
            "it blamed the character set: {why}"
        );
    }
}
