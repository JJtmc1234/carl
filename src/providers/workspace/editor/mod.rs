//! Opening and saving one file, without losing anybody's work.
//!
//! The only interesting thing a small editor has to get right is the case where the file
//! changed underneath it. JJ opens a file in the panel, an agent edits the same file on the
//! branch, JJ presses save, and an hour of somebody's work disappears with no error. So a save
//! compares what is on disk now against what was there when the file was opened, and refuses
//! rather than overwriting.
//!
//! Deliberately not an editor platform. No language server, no syntax engine, no plugin
//! surface. Reading text, writing text, and refusing to clobber. Syntax highlighting belongs to
//! whatever draws this, which already knows the file extension from `OpenFile::path`.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// The largest file that will be opened. Eight mebibytes.
///
/// A panel that tries to load a database dump into a text box stops being a panel. Refusing is
/// better than freezing, and the refusal says the size so it does not look like a bug.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// Whether the file may be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    ReadWrite,
    /// Opened to look at. A save is refused rather than silently doing nothing.
    ReadOnly,
}

/// Enough about a file to tell whether it is the one we read.
///
/// Length and modified time catch almost everything, and the hash catches the case they miss:
/// an edit that keeps the length and lands inside the same timestamp granularity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub len: u64,
    pub modified: Option<u64>,
    pub hash: u64,
}

impl Fingerprint {
    fn of(path: &Path, contents: &str) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        Ok(Self {
            len: meta.len(),
            modified,
            hash: hash(contents.as_bytes()),
        })
    }
}

/// FNV-1a, which is plenty for noticing a file changed and needs no dependency.
fn hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// A file the panel has open.
#[derive(Debug, Clone)]
pub struct OpenFile {
    path: PathBuf,
    text: String,
    mode: Mode,
    /// What the file looked like when it was read.
    seen: Fingerprint,
}

impl OpenFile {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The contents as they were read from disk.
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn is_read_only(&self) -> bool {
        self.mode == Mode::ReadOnly
    }

    /// What the file looked like when it was opened.
    pub fn fingerprint(&self) -> Fingerprint {
        self.seen
    }

    /// The extension, for whoever picks a syntax highlighter.
    pub fn extension(&self) -> Option<&str> {
        self.path.extension().and_then(|e| e.to_str())
    }

    /// Whether somebody else has touched the file since it was opened.
    ///
    /// A file that has been deleted counts as changed, because saving over the gap would
    /// recreate a file whose absence may have been the point.
    pub fn changed_on_disk(&self) -> bool {
        match read_text(&self.path) {
            Err(_) => true,
            Ok(now) => Fingerprint::of(&self.path, &now)
                .map(|f| f != self.seen)
                .unwrap_or(true),
        }
    }

    /// Writes `text`, refusing if the file changed since it was opened.
    ///
    /// The refusal is the feature. A caller that genuinely wants to overwrite can `reload`,
    /// show the difference, and save again, which is a decision somebody made rather than a
    /// race somebody lost.
    pub fn save(&mut self, text: &str) -> Result<()> {
        if self.mode == Mode::ReadOnly {
            return Err(Error::Refused(format!(
                "{} is open read only",
                self.path.display()
            )));
        }
        if self.changed_on_disk() {
            return Err(Error::Refused(format!(
                "{} changed on disk since it was opened. Reload and look at the difference \
                 before saving over it",
                self.path.display()
            )));
        }

        // Written beside the file and renamed, so an interrupted save cannot leave a truncated
        // file where a whole one used to be.
        let staging = self.path.with_extension(staging_extension(&self.path));
        std::fs::write(&staging, text)?;
        std::fs::rename(&staging, &self.path)?;

        self.text = text.to_string();
        self.seen = Fingerprint::of(&self.path, text)?;
        Ok(())
    }

    /// Rereads from disk, discarding what was held.
    pub fn reload(&mut self) -> Result<()> {
        let text = read_text(&self.path)?;
        self.seen = Fingerprint::of(&self.path, &text)?;
        self.text = text;
        Ok(())
    }
}

fn staging_extension(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) => format!("{e}.writing"),
        None => "writing".to_string(),
    }
}

/// Opens a file for the panel.
///
/// Refuses a directory, something too large to hold, and anything that is not text. The last
/// one matters: a text box handed arbitrary bytes shows mojibake and a save turns the file into
/// something the program that owns it can no longer read.
pub fn open(path: impl AsRef<Path>, mode: Mode) -> Result<OpenFile> {
    let path = path.as_ref();

    let meta = std::fs::metadata(path)
        .map_err(|e| Error::Refused(format!("cannot open {}: {e}", path.display())))?;
    if meta.is_dir() {
        return Err(Error::Refused(format!("{} is a directory", path.display())));
    }
    if meta.len() > MAX_BYTES {
        return Err(Error::Refused(format!(
            "{} is {} bytes, larger than the {MAX_BYTES} this opens",
            path.display(),
            meta.len()
        )));
    }

    // Canonical so that two paths to the same file are one file, which is what makes the stale
    // check trustworthy when a project is reached through a symlink.
    let path = path
        .canonicalize()
        .map_err(|e| Error::Refused(format!("cannot resolve {}: {e}", path.display())))?;

    let text = read_text(&path)?;
    let seen = Fingerprint::of(&path, &text)?;
    Ok(OpenFile {
        path,
        text,
        mode,
        seen,
    })
}

fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| Error::Refused(format!("cannot read {}: {e}", path.display())))?;
    String::from_utf8(bytes).map_err(|_| Error::Refused(format!("{} is not text", path.display())))
}

#[cfg(test)]
mod tests;
