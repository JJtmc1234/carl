//! The file the record is kept in, and the one rule that makes it countable.
//!
//! Append only, one JSON object per line, flushed before the call returns. Buffered would be
//! faster and would lose exactly the lines that matter, since the whole value of this file is
//! that it survives whatever happens next.
//!
//! **More than one thing writes it, and that is what the lock is for.** The supervisor records
//! that a process started. Carl records that work was handed down. Those are two processes with
//! two sequence counters over one file, and two counters that agree today both hand out seq 41
//! the first time they write in the same second. A reader replaying the file then sees two
//! different events claiming the same place in the order, which is the one thing a sequence
//! number exists to rule out.
//!
//! So every append takes an exclusive `flock` on the file, and inside that lock it checks
//! whether the file grew since it last wrote. If it did, somebody else has written and the
//! numbering is reread from what they left. The lock is released by closing the file, which
//! happens whether the write worked, failed, or panicked.
//!
//! **Reread from where we stopped, not from the beginning.** Only whole lines are ever written,
//! and only under the lock, so the bytes after the point this journal last saw are whole lines.
//! A file that somehow got shorter is reread from the start instead, because something replaced
//! it and nothing about the old position means anything any more.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use super::{Event, Record};
use crate::Result;
use crate::army::task::TaskId;

/// The record itself.
pub struct Journal {
    path: PathBuf,
    next_seq: u64,
    /// How long the file was when this journal last knew the numbering was right.
    ///
    /// Not a cache of the contents. It is the answer to one question, "has anybody else written
    /// since I did", and it is the whole reason an append does not have to reread the file.
    seen: u64,
}

impl Journal {
    /// Opens the record, continuing the numbering where it left off.
    ///
    /// Reading the whole file to find the last sequence number is fine at this size and wrong at
    /// a million lines, and it is written down here so whoever hits that knows it was a choice.
    ///
    /// **One read, and the length comes out of it rather than out of a second look at the file.**
    /// Reading the contents and then asking how long the file is leaves a gap, and anything
    /// written in that gap is counted in the length but not in the numbering. The journal then
    /// believes it is up to date when it is a line behind, skips the catch up on its first
    /// append, and reissues a number that is already in the file. That is not a rare race: two
    /// journals opened at once over an empty file hit it whenever one of them writes first.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };

        Ok(Self {
            path,
            next_seq: highest(&text).map_or(1, |seq| seq + 1),
            seen: text.len() as u64,
        })
    }

    /// Writes one line, and flushes it before returning.
    pub fn append(&mut self, actor: &str, event: Event) -> Result<Record> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?;

        let _held = Exclusive::take(&file)?;
        self.catch_up(&mut file)?;

        let record = Record {
            seq: self.next_seq,
            at: now(),
            actor: actor.to_string(),
            event,
        };

        writeln!(file, "{}", serde_json::to_string(&record)?)?;
        file.flush()?;

        self.next_seq += 1;
        self.seen = file.metadata().map(|m| m.len()).unwrap_or(self.seen);
        Ok(record)
    }

    /// Reads the whole record and appends what the caller decides from it, both under one lock.
    ///
    /// For the writer whose line depends on what is already there. "Accept this task if it is
    /// still waiting on review" as two calls leaves a gap between the reading and the writing,
    /// and that gap is exactly where two processes both find a task waiting and both accept it.
    /// Held open across the decision instead, so whoever gets the lock second reads the first
    /// one's line and refuses.
    ///
    /// The decision may say no, and saying no is not an error: a caller that finds the work
    /// already done has got the answer it asked for.
    pub fn decide_and_append(
        &mut self,
        decide: impl FnOnce(&[Record]) -> Result<Option<(String, Event)>>,
    ) -> Result<Option<Record>> {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(&self.path)?;

        let _held = Exclusive::take(&file)?;
        self.catch_up(&mut file)?;

        let mut whole = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut whole)?;
        let records: Vec<Record> = whole
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        let Some((actor, event)) = decide(&records)? else {
            return Ok(None);
        };

        let record = Record {
            seq: self.next_seq,
            at: now(),
            actor,
            event,
        };
        writeln!(file, "{}", serde_json::to_string(&record)?)?;
        file.flush()?;

        self.next_seq += 1;
        self.seen = file.metadata().map(|m| m.len()).unwrap_or(self.seen);
        Ok(Some(record))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Takes the numbering from whatever anybody else wrote while this journal was not looking.
    ///
    /// Only ever called holding the lock, which is what makes the answer still true by the time
    /// it is used.
    fn catch_up(&mut self, file: &mut File) -> Result<()> {
        let len = file.metadata()?.len();
        if len == self.seen {
            return Ok(());
        }

        // Shorter than it was means something replaced the file rather than appended to it, and
        // the position this journal remembers is about a file that no longer exists.
        let from = if len < self.seen { 0 } else { self.seen };

        file.seek(SeekFrom::Start(from))?;
        let mut tail = String::new();
        file.read_to_string(&mut tail)?;

        if let Some(seq) = highest(&tail) {
            self.next_seq = self.next_seq.max(seq + 1);
        }
        self.seen = len;
        Ok(())
    }
}

/// The largest sequence number in some lines of the record.
///
/// Largest rather than last, because a line that will not parse is skipped and the last one that
/// does may not be the last one written.
fn highest(lines: &str) -> Option<u64> {
    lines
        .lines()
        .filter_map(|l| serde_json::from_str::<Record>(l).ok())
        .map(|r| r.seq)
        .max()
}

/// An exclusive lock on the file, released when it goes out of scope.
///
/// A guard rather than a pair of calls, because the release has to happen on the path where the
/// write failed as well as the one where it worked, and a lock left held by an error return is a
/// journal that nothing else can ever write again.
struct Exclusive(i32);

impl Exclusive {
    /// Waits for the lock rather than giving up on it.
    ///
    /// A writer that gave up would have to either drop the line or invent a number, and both of
    /// those are worse than waiting for a write that takes microseconds.
    fn take(file: &File) -> Result<Self> {
        let fd = file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(fd))
    }
}

impl Drop for Exclusive {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0, libc::LOCK_UN) };
    }
}

/// Everything recorded, in order.
///
/// A line that cannot be read is skipped rather than fatal. A record with one corrupt line is
/// still worth reading, and refusing to open it would lose everything else in it.
pub fn read(path: impl AsRef<Path>) -> Result<Vec<Record>> {
    let text = match std::fs::read_to_string(path.as_ref()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };

    Ok(text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect())
}

/// Everything about one task, in order.
pub fn about(records: &[Record], task: &TaskId) -> Vec<Record> {
    records
        .iter()
        .filter(|r| r.event.task() == Some(task))
        .cloned()
        .collect()
}

/// Unix seconds. `pub(crate)` because the chain writes the same clock into agent folders, and two
/// clocks would let a journal entry and a state file disagree about when something happened.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
