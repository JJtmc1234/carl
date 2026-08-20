//! Which Claude Code session belongs to which conversation.
//!
//! Back and forth within one conversation is not something Carl has to build. Claude Code
//! already keeps a session transcript and `--resume` continues it. What Carl has to do is
//! remember which session belongs to which Slack thread, so the second message in a thread
//! lands in the same conversation as the first.
//!
//! The registry is a plain JSON file. It is small, it is read once per message, and it is the
//! only thing standing between a thread and its history, so it is written before the session
//! is used rather than after.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, ThreadId};

/// A Claude Code session id. Must be a valid UUID, because `--session-id` requires one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// A fresh random v4 UUID, from the kernel.
    ///
    /// Read from `/dev/urandom` rather than pulling in a uuid crate. It is the same source
    /// the crates use and it keeps the dependency list short.
    pub fn fresh() -> Result<Self> {
        use std::io::Read;
        let mut b = [0u8; 16];
        std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;

        // Set the version and variant bits so this is a well formed v4 UUID. Claude Code
        // validates the shape, so a bare 32 hex characters is refused.
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;

        let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
        Ok(Self(format!(
            "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
            h[0],
            h[1],
            h[2],
            h[3],
            h[4],
            h[5],
            h[6],
            h[7],
            h[8],
            h[9],
            h[10],
            h[11],
            h[12],
            h[13],
            h[14],
            h[15]
        )))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Thread {
    pub session: SessionId,
    /// Unix seconds when this thread was first seen.
    pub started_at: u64,
    /// How many times Carl has answered in it. Useful for spotting a runaway loop.
    #[serde(default)]
    pub turns: u64,
    /// Unix seconds of the last answer.
    ///
    /// Defaulted rather than required, so a registry written before this existed still loads
    /// and simply looks like it has never been used, which for warming is the safe reading.
    #[serde(default)]
    pub last_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Registry {
    #[serde(default)]
    threads: BTreeMap<ThreadId, Thread>,
    #[serde(skip)]
    path: PathBuf,
}

impl Registry {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let mut registry: Self = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| {
                Error::Refused(format!("{} is not a valid registry: {e}", path.display()))
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(e.into()),
        };
        registry.path = path;
        Ok(registry)
    }

    /// The session for this thread, minting one on first sight.
    ///
    /// Written to disk before it is returned. If the process dies between minting and using,
    /// the next run finds the same session rather than starting a second conversation for a
    /// thread that already has one.
    pub fn session_for(&mut self, thread: &ThreadId, now: u64) -> Result<(SessionId, bool)> {
        if let Some(existing) = self.threads.get(thread) {
            return Ok((existing.session.clone(), false));
        }

        let session = SessionId::fresh()?;
        self.threads.insert(
            thread.clone(),
            Thread {
                session: session.clone(),
                started_at: now,
                turns: 0,
                last_at: now,
            },
        );
        self.save()?;
        Ok((session, true))
    }

    /// Whether claude has ever finished a turn in this thread, so there is something to resume.
    ///
    /// Not the same question as whether the registry has an entry, which is what the resume
    /// decision used to be made on. `session_for` mints and saves the id before claude is ever
    /// contacted, so a thread whose very first turn failed keeps an entry naming an id that
    /// nothing on the far side has ever seen. Every later turn then asked with `--resume` for
    /// that id, claude answered "No conversation found with session ID", and the thread was
    /// wedged permanently.
    ///
    /// `turns` is the right field because `record_turn` runs only on the success path, so it
    /// already counts exactly the thing this needs to know. See bug 17.
    pub fn has_transcript(&self, thread: &ThreadId) -> bool {
        self.threads.get(thread).is_some_and(|t| t.turns > 0)
    }

    pub fn record_turn(&mut self, thread: &ThreadId) -> Result<()> {
        if let Some(entry) = self.threads.get_mut(thread) {
            entry.turns += 1;
            entry.last_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(entry.last_at);
        }
        self.save()
    }

    /// The threads used most recently, newest first.
    ///
    /// Used to decide what to reopen after a restart. Restarting loses every held open
    /// process, so without this the first message to each thread pays the full startup again,
    /// and the person who notices is whoever was mid conversation when it happened.
    pub fn recent(&self, limit: usize) -> Vec<(ThreadId, SessionId)> {
        let mut all: Vec<(&ThreadId, &Thread)> = self.threads.iter().collect();
        all.sort_by_key(|(_, t)| std::cmp::Reverse(t.last_at.max(t.started_at)));
        all.into_iter()
            .filter(|(_, t)| t.turns > 0)
            .take(limit)
            .map(|(id, t)| (id.clone(), t.session.clone()))
            .collect()
    }

    pub fn get(&self, thread: &ThreadId) -> Option<&Thread> {
        self.threads.get(thread)
    }

    pub fn len(&self) -> usize {
        self.threads.len()
    }

    pub fn is_empty(&self) -> bool {
        self.threads.is_empty()
    }

    /// Writes to a neighbouring temporary file and renames, so a crash mid write cannot
    /// leave a half written registry that loses every thread's history.
    fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = self.path.with_extension("json.writing");
        std::fs::write(&staging, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&staging, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Restarting loses every held open process, so the ones worth reopening are the ones
    /// somebody was most recently talking in.
    #[test]
    fn the_most_recently_used_threads_come_back_first() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Registry::open(d.path().join("threads.json")).unwrap();

        for (name, at) in [("old", 100), ("newer", 200), ("newest", 300)] {
            let t = ThreadId::new(name).unwrap();
            r.session_for(&t, at).unwrap();
            r.record_turn(&t).unwrap();
            // record_turn stamps with the clock, so the ordering is set by hand here.
            r.threads.get_mut(&t).unwrap().last_at = at;
        }

        let names: Vec<String> = r
            .recent(2)
            .into_iter()
            .map(|(t, _)| t.to_string())
            .collect();
        assert_eq!(names, vec!["newest", "newer"]);
    }

    /// A thread that was created and never used is not worth a process. Every Slack mention
    /// creates one, and most of them are asked a single question or none at all.
    #[test]
    fn a_thread_nobody_ever_answered_in_is_not_warmed() {
        let d = tempfile::tempdir().unwrap();
        let mut r = Registry::open(d.path().join("threads.json")).unwrap();
        r.session_for(&ThreadId::new("never-used").unwrap(), 100)
            .unwrap();

        assert!(r.recent(4).is_empty());
    }

    fn t(name: &str) -> ThreadId {
        ThreadId::new(name).unwrap()
    }

    #[test]
    fn a_uuid_is_well_formed() {
        let id = SessionId::fresh().unwrap();
        let s = id.as_str();
        assert_eq!(s.len(), 36, "{s}");
        assert_eq!(
            s.chars().filter(|c| *c == '-').count(),
            4,
            "wrong dash count in {s}"
        );
        // Version 4, variant 8/9/a/b. Claude Code rejects anything else.
        assert_eq!(s.as_bytes()[14] as char, '4', "{s}");
        assert!("89ab".contains(s.as_bytes()[19] as char), "{s}");
    }

    #[test]
    fn ids_do_not_repeat() {
        let ids: std::collections::BTreeSet<_> =
            (0..200).map(|_| SessionId::fresh().unwrap().0).collect();
        assert_eq!(ids.len(), 200);
    }

    /// The whole point. The same thread must come back to the same conversation.
    #[test]
    fn a_thread_keeps_its_session_across_restarts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("threads.json");

        let mut registry = Registry::open(&path).unwrap();
        let (first, minted) = registry.session_for(&t("slack-C1-1700"), 100).unwrap();
        assert!(minted, "the first sighting mints a session");

        let (again, minted) = registry.session_for(&t("slack-C1-1700"), 200).unwrap();
        assert!(!minted, "the second sighting reuses it");
        assert_eq!(first, again);

        // A whole new process, reading the file from scratch.
        let mut reopened = Registry::open(&path).unwrap();
        let (after_restart, minted) = reopened.session_for(&t("slack-C1-1700"), 300).unwrap();
        assert!(!minted);
        assert_eq!(
            first, after_restart,
            "a restart must not orphan the history"
        );
    }

    #[test]
    fn different_threads_get_different_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::open(dir.path().join("threads.json")).unwrap();

        let (a, _) = registry.session_for(&t("slack-C1-1700"), 100).unwrap();
        let (b, _) = registry.session_for(&t("slack-C1-1800"), 100).unwrap();
        assert_ne!(a, b, "two Slack threads must not share one conversation");
    }

    #[test]
    fn a_session_is_on_disk_before_it_is_returned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("threads.json");

        let (session, _) = Registry::open(&path)
            .unwrap()
            .session_for(&t("crashy"), 100)
            .unwrap();

        // Nothing called save explicitly, and the file is already correct.
        let on_disk = Registry::open(&path).unwrap();
        assert_eq!(on_disk.get(&t("crashy")).unwrap().session, session);
    }

    #[test]
    fn no_staging_file_is_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("threads.json");
        Registry::open(&path)
            .unwrap()
            .session_for(&t("a"), 1)
            .unwrap();

        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["threads.json".to_string()]);
    }

    #[test]
    fn a_corrupt_registry_is_an_error_rather_than_a_silent_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("threads.json");
        std::fs::write(&path, "{not json").unwrap();

        // Starting fresh here would silently orphan every thread's history.
        assert!(Registry::open(&path).is_err());
    }
}
