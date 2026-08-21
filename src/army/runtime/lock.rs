//! One supervisor per home, enforced rather than assumed.
//!
//! Two supervisors on the same home is not a small problem. Each reads the other's records, finds
//! processes it does not own, decides they are orphans from a dead supervisor, and kills them. The
//! other does the same. The visible symptom is agents that keep restarting for no reason, and the
//! cause is invisible because both supervisors are behaving exactly as designed.
//!
//! Which is close to certain to happen, because the way this gets run is `carl supervise` in a
//! terminal, and later a systemd unit doing the same thing. Nobody will remember the terminal.
//!
//! **A pid file, with the start time in it.** A pid alone is the classic broken version: the
//! supervisor is killed, the file stays, the pid is reused by something unrelated, and the lock
//! is held forever by a process that has never heard of Carl. With the start time as well, a
//! stale file is recognised as stale and taken over, and a live one is never taken over.

use std::path::{Path, PathBuf};

use crate::providers::system::started;
use crate::{Error, Result};

/// What the lock file says.
///
/// Plain text, two numbers, because the thing most likely to read this is a person wondering why
/// their supervisor will not start.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Held {
    pid: u32,
    started: u64,
}

/// The claim on one home. Released when dropped.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    pid: u32,
}

impl Lock {
    /// Takes the lock for this process, or says who has it.
    pub fn take(home: &Path) -> Result<Self> {
        let path = home.join("run").join("supervisor.pid");
        std::fs::create_dir_all(path.parent().expect("run has a parent"))?;

        if let Some(held) = read(&path)
            && started::is_still(held.pid, held.started)
        {
            return Err(Error::Refused(format!(
                "a supervisor is already running on this home as process {}. Two would fight \
                 over every agent, each ending the other's processes as orphans",
                held.pid
            )));
        }

        let pid = std::process::id();
        let started = started::started(pid)
            .ok_or_else(|| Error::Refused("cannot read this process's own start time".into()))?;
        std::fs::write(&path, format!("{pid} {started}\n"))?;

        Ok(Self { path, pid })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Lock {
    /// Releases the lock, but only if it is still ours.
    ///
    /// A supervisor that was killed and whose lock was taken over by a successor must not have
    /// its Drop, if it ever runs, remove the successor's claim.
    fn drop(&mut self) {
        if read(&self.path).is_some_and(|held| held.pid == self.pid) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Reads the lock file, or nothing if it is absent or unreadable.
///
/// A file that cannot be understood is treated as no lock at all. The alternative is a supervisor
/// that will not start because of one corrupt line, which is a worse failure than the one this
/// module exists to prevent.
fn read(path: &Path) -> Option<Held> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut parts = text.split_whitespace();
    Some(Held {
        pid: parts.next()?.parse().ok()?,
        started: parts.next()?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_the_lock_writes_this_process_and_releases_on_drop() {
        let d = tempfile::tempdir().unwrap();
        let lock = Lock::take(d.path()).unwrap();
        let path = lock.path().to_path_buf();

        assert_eq!(read(&path).unwrap().pid, std::process::id());
        drop(lock);
        assert!(!path.exists(), "released");
    }

    /// The failure this exists for. Both supervisors would keep working, and would spend the
    /// night ending each other's agents as orphans.
    #[test]
    fn a_second_supervisor_on_the_same_home_is_refused_and_says_who_has_it() {
        let d = tempfile::tempdir().unwrap();
        let _first = Lock::take(d.path()).unwrap();

        let err = Lock::take(d.path()).unwrap_err().to_string();
        assert!(err.contains(&std::process::id().to_string()), "{err}");
    }

    /// A supervisor killed with SIGKILL never runs its Drop. Without the start time the pid it
    /// left behind would eventually belong to something unrelated and hold the lock forever.
    #[test]
    fn a_lock_left_by_a_process_that_is_gone_is_taken_over() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("run").join("supervisor.pid");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{} 12345\n", u32::MAX)).unwrap();

        let lock = Lock::take(d.path()).unwrap();
        assert_eq!(read(lock.path()).unwrap().pid, std::process::id());
    }

    /// The same pid, a different process. Recognised as stale rather than believed, which a
    /// bare pid file cannot do.
    #[test]
    fn a_lock_naming_this_pid_with_the_wrong_start_time_is_stale() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("run").join("supervisor.pid");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mine = started::started(std::process::id()).unwrap();
        std::fs::write(&path, format!("{} {}\n", std::process::id(), mine + 1)).unwrap();

        assert!(Lock::take(d.path()).is_ok());
    }

    /// One corrupt line refusing to let the army start would be a worse fault than the one
    /// this module prevents.
    #[test]
    fn an_unreadable_lock_file_is_no_lock_at_all() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("run").join("supervisor.pid");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "who knows\n").unwrap();

        assert!(Lock::take(d.path()).is_ok());
    }

    /// A supervisor whose lock was taken over must not remove its successor's claim, whenever
    /// its own Drop happens to run.
    #[test]
    fn dropping_a_lock_that_was_taken_over_leaves_the_new_one_alone() {
        let d = tempfile::tempdir().unwrap();
        let stale = Lock {
            path: d.path().join("run").join("supervisor.pid"),
            pid: 999_999,
        };
        std::fs::create_dir_all(stale.path.parent().unwrap()).unwrap();
        std::fs::write(&stale.path, "12345 678\n").unwrap();

        let path = stale.path.clone();
        drop(stale);
        assert!(path.exists(), "somebody else's lock is not ours to remove");
    }
}
