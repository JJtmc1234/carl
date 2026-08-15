//! One handle over the terminal, the editor and the diff.
//!
//! The pieces underneath already work. What was missing was somewhere for a user interface to
//! hold them: a session it can name, keep across frames, and close. That is all this is. It
//! owns the sessions, hands out ids, and forwards. It is not a remote procedure call framework
//! and should not grow into one.
//!
//! **Investigation is a lookup and nothing else.** A panel row carries a component id, and
//! clicking it asks for the detail behind that id. The id is used as a key into a snapshot that
//! was already taken. It is never run, never interpolated into a command, and never turned into
//! a path. A component called `$(whoami)` or `; rm -rf ~` finds nothing, which is the only
//! thing it should ever do.
//!
//! **Still JJ's.** Nothing here is registered as a tool or reachable from `claude::Runner`. An
//! agent that wanted a shell would have to be given one deliberately, somewhere else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::providers::diagnostics::Snapshot;
use crate::providers::health::{Health, Kind};
use crate::{Error, Result};

use super::diff;
use super::editor::{self, Mode, OpenFile};
use super::terminal::{Size, Terminal};

/// Names one open thing. Handed out by the workspace and meaningless elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(u64);

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

/// What a panel wants to show about an open file without reading it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub path: PathBuf,
    pub bytes: usize,
    pub lines: usize,
    pub read_only: bool,
    pub extension: Option<String>,
    /// Whether somebody else has touched it since it was opened.
    pub changed_on_disk: bool,
}

/// The detail behind one diagnostic row.
#[derive(Debug, Clone, PartialEq)]
pub struct Investigation {
    pub component: String,
    pub group: String,
    pub health: Health,
    pub summary: String,
    pub metrics: Vec<(String, String)>,
    pub measured_at: Option<u64>,
    pub kind: Kind,
}

/// JJ's open terminals, files and comparisons.
#[derive(Default)]
pub struct Workspace {
    terminals: BTreeMap<SessionId, Terminal>,
    editors: BTreeMap<SessionId, OpenFile>,
    next: u64,
}

impl Workspace {
    pub fn new() -> Self {
        Self::default()
    }

    fn mint(&mut self) -> SessionId {
        self.next += 1;
        SessionId(self.next)
    }

    // ---- terminal ----

    /// Starts a shell in `cwd`.
    pub fn open_terminal(&mut self, cwd: impl AsRef<Path>, size: Size) -> Result<SessionId> {
        let terminal = Terminal::open(cwd, size)?;
        let id = self.mint();
        self.terminals.insert(id, terminal);
        Ok(id)
    }

    pub fn terminals(&self) -> Vec<SessionId> {
        self.terminals.keys().copied().collect()
    }

    /// Sends what JJ typed, exactly as typed.
    pub fn input(&mut self, id: SessionId, text: &str) -> Result<()> {
        self.terminal_mut(id)?.send(text)
    }

    pub fn input_line(&mut self, id: SessionId, line: &str) -> Result<()> {
        self.terminal_mut(id)?.send_line(line)
    }

    pub fn resize(&mut self, id: SessionId, size: Size) -> Result<()> {
        self.terminal_mut(id)?.resize(size)
    }

    /// Everything printed since the last call.
    pub fn drain(&mut self, id: SessionId) -> Result<Vec<u8>> {
        Ok(self.terminal_mut(id)?.drain())
    }

    /// Where the shell is now, which is what the panel shows.
    pub fn cwd(&self, id: SessionId) -> Option<PathBuf> {
        self.terminals.get(&id)?.current_dir()
    }

    pub fn is_alive(&mut self, id: SessionId) -> bool {
        self.terminals.get_mut(&id).is_some_and(|t| t.is_alive())
    }

    /// Ends the session and forgets it.
    pub fn close_terminal(&mut self, id: SessionId) -> Result<()> {
        match self.terminals.remove(&id) {
            Some(mut t) => t.close(),
            None => Err(Error::Refused(format!("{id} is not an open terminal"))),
        }
    }

    /// Forgets every terminal whose shell has exited, returning which were removed.
    ///
    /// A dead session is kept until somebody asks, so a panel can draw the fact that a shell
    /// exited rather than having the row vanish underneath JJ. That is the right default and
    /// it also means the table would grow for the life of the process if nothing ever cleared
    /// it, so this is the clearing. A panel is expected to call it when it redraws.
    pub fn reap(&mut self) -> Vec<SessionId> {
        // A loop rather than a filter, because asking whether a child is alive reaps it and so
        // needs it mutably, which a filter closure cannot hand out.
        let mut dead: Vec<SessionId> = Vec::new();
        for (id, terminal) in self.terminals.iter_mut() {
            if !terminal.is_alive() {
                dead.push(*id);
            }
        }

        for id in &dead {
            if let Some(mut terminal) = self.terminals.remove(id) {
                // Already exited, so this only reaps the entry rather than killing anything.
                let _ = terminal.close();
            }
        }
        dead
    }

    /// How many sessions are being held, for a caller watching for a leak.
    pub fn held(&self) -> (usize, usize) {
        (self.terminals.len(), self.editors.len())
    }

    fn terminal_mut(&mut self, id: SessionId) -> Result<&mut Terminal> {
        self.terminals
            .get_mut(&id)
            .ok_or_else(|| Error::Refused(format!("{id} is not an open terminal")))
    }

    // ---- editor ----

    pub fn open_file(&mut self, path: impl AsRef<Path>, mode: Mode) -> Result<SessionId> {
        let file = editor::open(path, mode)?;
        let id = self.mint();
        self.editors.insert(id, file);
        Ok(id)
    }

    pub fn files(&self) -> Vec<SessionId> {
        self.editors.keys().copied().collect()
    }

    /// The contents as last read from disk.
    pub fn contents(&self, id: SessionId) -> Option<&str> {
        self.editors.get(&id).map(|f| f.text())
    }

    /// Writes `text`, refusing if the file changed since it was opened.
    pub fn save(&mut self, id: SessionId, text: &str) -> Result<()> {
        self.editor_mut(id)?.save(text)
    }

    pub fn reload(&mut self, id: SessionId) -> Result<()> {
        self.editor_mut(id)?.reload()
    }

    pub fn changed_on_disk(&self, id: SessionId) -> Option<bool> {
        Some(self.editors.get(&id)?.changed_on_disk())
    }

    pub fn file_info(&self, id: SessionId) -> Option<FileInfo> {
        let file = self.editors.get(&id)?;
        Some(FileInfo {
            path: file.path().to_path_buf(),
            bytes: file.text().len(),
            lines: file.text().lines().count(),
            read_only: file.is_read_only(),
            extension: file.extension().map(str::to_string),
            changed_on_disk: file.changed_on_disk(),
        })
    }

    pub fn close_file(&mut self, id: SessionId) -> Result<()> {
        match self.editors.remove(&id) {
            Some(_) => Ok(()),
            None => Err(Error::Refused(format!("{id} is not an open file"))),
        }
    }

    fn editor_mut(&mut self, id: SessionId) -> Result<&mut OpenFile> {
        self.editors
            .get_mut(&id)
            .ok_or_else(|| Error::Refused(format!("{id} is not an open file")))
    }

    // ---- diff ----

    /// What an open file looks like against the commit its branch is on.
    pub fn diff_against_head(&self, id: SessionId) -> Result<String> {
        let file = self
            .editors
            .get(&id)
            .ok_or_else(|| Error::Refused(format!("{id} is not an open file")))?;
        let repo = diff::repository_of(file.path()).ok_or_else(|| {
            Error::Refused(format!(
                "{} is not inside a git repository",
                file.path().display()
            ))
        })?;
        diff::against_head(&repo, file.path())
    }

    /// What is in the editor against what is on disk, which git cannot see.
    pub fn diff_buffer(&self, id: SessionId, buffer: &str) -> Result<String> {
        let file = self
            .editors
            .get(&id)
            .ok_or_else(|| Error::Refused(format!("{id} is not an open file")))?;
        Ok(diff::buffer_vs_disk(file.text(), buffer))
    }

    /// Everything changed in a working tree.
    pub fn worktree_changes(&self, repo: impl AsRef<Path>) -> Result<Vec<diff::Change>> {
        diff::worktree_changes(repo.as_ref())
    }

    /// A one line summary of a working tree.
    pub fn worktree_summary(&self, repo: impl AsRef<Path>) -> Result<String> {
        diff::summarise(repo.as_ref())
    }

    // ---- investigation ----

    /// The detail behind a diagnostic row.
    ///
    /// `component` is a key and only a key. It is compared against ids already present in a
    /// snapshot that has already been taken, so an unknown or hostile looking string finds
    /// nothing and causes nothing to happen.
    pub fn investigate(&self, snapshot: &Snapshot, component: &str) -> Option<Investigation> {
        let found = snapshot.find(component)?;
        Some(Investigation {
            component: found.component.clone(),
            group: found.group().to_string(),
            health: found.health,
            summary: found.summary.clone(),
            metrics: found.metric_pairs(),
            measured_at: found.measured_at,
            kind: found.kind,
        })
    }
}

impl Drop for Workspace {
    /// A closed panel leaves no shells running.
    fn drop(&mut self) {
        for (_, mut terminal) in std::mem::take(&mut self.terminals) {
            let _ = terminal.close();
        }
    }
}

#[cfg(test)]
mod tests;
