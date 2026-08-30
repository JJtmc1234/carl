//! Getting a line onto disk, and keeping the file from growing forever.

use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;

use super::{Line, MOST_BYTES};

/// Appends one line and trims the file if it has grown past its cap.
///
/// Locked the same way the journal is, because the supervisor, Carl and the panel all write
/// here and the trim rewrites the whole file. Two writers trimming at once over one file is how
/// a log loses the half somebody was reading.
pub(super) fn append(path: &Path, line: &Line) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)?;

    let _held = Held::take(&file)?;
    let text = serde_json::to_string(line).map_err(std::io::Error::other)?;
    writeln!(file, "{text}")?;
    file.flush()?;

    if file.metadata()?.len() > MOST_BYTES {
        trim(&mut file)?;
    }
    Ok(())
}

/// Drops the older half. Called holding the lock.
///
/// Half rather than one line at a time, so trimming happens rarely instead of on every append
/// once the cap is reached. The file is opened for appending, so a write after `set_len(0)`
/// lands at the start whatever the seek position says.
fn trim(file: &mut std::fs::File) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut text = String::new();
    file.seek(SeekFrom::Start(0))?;
    file.read_to_string(&mut text)?;

    let lines: Vec<&str> = text.lines().collect();
    let kept = lines[lines.len() / 2..].join("\n");

    file.set_len(0)?;
    writeln!(file, "{kept}")?;
    file.flush()
}

/// An exclusive lock that is released by being dropped.
struct Held(i32);

impl Held {
    fn take(file: &std::fs::File) -> std::io::Result<Self> {
        let fd = file.as_raw_fd();
        if unsafe { libc::flock(fd, libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(fd))
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0, libc::LOCK_UN) };
    }
}
