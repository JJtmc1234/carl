//! What happened, in a form somebody can ask questions of later.
//!
//! Append only, one JSON object per line, and the same rule the rest of Carl already follows:
//! write it down before acting on it. A crash after recording loses the outcome, which can be
//! looked up. A crash before recording loses the fact that anything was attempted, and nothing
//! recovers that.
//!
//! The reason this exists rather than logging is that a hierarchy is only worth having if
//! somebody can check it was followed. Twenty agents producing work with no record is twenty
//! opinions and a story about where they came from. With a record, "who approved this", "how
//! many times did she try", and "did anybody actually review it" are questions with answers.
//!
//! Refusals are recorded as well as actions, and they are the interesting half. A log holding
//! only what happened cannot answer what somebody tried to do and was stopped from doing,
//! which is the question worth asking when something has gone wrong.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::task::{Status, TaskId};
use crate::{ProjectId, Result};

/// Something worth being able to ask about later.
///
/// Deliberately closed rather than a free string. A string means every writer invents its own
/// wording and no reader can count anything, and counting is most of what a record is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Work handed from one agent to a direct report.
    ///
    /// Carries the parent and the verification conditions because a task is never written to
    /// disk anywhere else. This line is the only durable evidence the task exists, so anything
    /// a reader needs in order to rebuild it has to be here or it is gone when the process ends.
    /// Both fields default, so lines written before they existed still read.
    Delegated {
        task: TaskId,
        to: String,
        goal: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<TaskId>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        must: Vec<String>,
        /// Which project the work belongs to, when it belongs to one.
        ///
        /// Defaults, so every line written before projects existed still reads, as `None`. An
        /// old journal is not a broken journal, and refusing to open one would throw away the
        /// only record of everything that happened before this field did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        project: Option<ProjectId>,
    },
    /// A task moved from one state to another.
    Moved {
        task: TaskId,
        from: String,
        to: String,
    },
    /// An agent produced something for its task.
    Submitted {
        task: TaskId,
        attempt: u32,
        words: usize,
    },
    /// A review decided.
    Reviewed {
        task: TaskId,
        accepted: bool,
        why: String,
    },
    /// Something was refused, and by what rule.
    ///
    /// The interesting half of the record. Without it nobody can tell a rule that is working
    /// from a rule nothing has ever hit.
    Refused { what: String, why: String },
    /// A lead was allowed to implement, which normally it may not.
    ///
    /// Recorded separately from everything else because it is the one exception to the rank
    /// rules, and an exception nobody can count is an exception that becomes the habit.
    EmergencyDeclared { task: TaskId, why: String },
    /// A department or the chief said what it decided, having read what came back.
    Decided { task: Option<TaskId>, what: String },
    /// JJ reached past the chain of command and did something himself.
    ///
    /// Its own variant rather than a normal event with `actor: "jj"`, because the difference
    /// between "Mason reassigned Nora's task" and "JJ reassigned Nora's task over Mason's head"
    /// is the whole point of having a chain. Recording the second as though it were the first
    /// would make the record lie about who decided, which is the one thing it exists to answer.
    ///
    /// JJ has absolute authority, so this is never refused. It is only ever made visible.
    Intervened { what: Intervention },
    /// Somebody was told about something they did not do.
    ///
    /// Points at the sequence number of what they are being told about rather than repeating it,
    /// so a notification can never come to disagree with the thing it notifies about.
    Notified { who: String, about: u64 },
}

/// What JJ did, in a form a reader can count rather than parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intervention", rename_all = "snake_case")]
pub enum Intervention {
    /// A message straight to one agent, going around its lead.
    Message { to: String, what: String },
    /// A new objective for Carl. The ordinary way in, and still recorded.
    Objective { what: String },
    /// An answer to something Carl asked JJ to decide.
    Answered { question: String, answer: String },
    /// A task stopped where it stands.
    Stopped { task: TaskId, why: String },
    /// A task stopped and replaced with a different goal.
    Replaced {
        task: TaskId,
        goal: String,
        why: String,
    },
    /// A standing instruction to one agent that overrides what its lead told it.
    Override { agent: String, instruction: String },
}

impl Intervention {
    /// The agent this reached past the chain to touch, when there is one.
    pub fn agent(&self) -> Option<&str> {
        match self {
            Intervention::Message { to, .. } => Some(to),
            Intervention::Override { agent, .. } => Some(agent),
            Intervention::Objective { .. } | Intervention::Answered { .. } => None,
            Intervention::Stopped { .. } | Intervention::Replaced { .. } => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Intervention::Message { .. } => "message",
            Intervention::Objective { .. } => "objective",
            Intervention::Answered { .. } => "answered",
            Intervention::Stopped { .. } => "stopped",
            Intervention::Replaced { .. } => "replaced",
            Intervention::Override { .. } => "override",
        }
    }
}

impl Event {
    /// A short name, for counting without matching every variant.
    pub fn kind(&self) -> &'static str {
        match self {
            Event::Delegated { .. } => "delegated",
            Event::Moved { .. } => "moved",
            Event::Submitted { .. } => "submitted",
            Event::Reviewed { .. } => "reviewed",
            Event::Refused { .. } => "refused",
            Event::EmergencyDeclared { .. } => "emergency_declared",
            Event::Decided { .. } => "decided",
            Event::Intervened { .. } => "intervened",
            Event::Notified { .. } => "notified",
        }
    }

    /// The task this concerns, when it concerns one.
    pub fn task(&self) -> Option<&TaskId> {
        match self {
            Event::Delegated { task, .. }
            | Event::Moved { task, .. }
            | Event::Submitted { task, .. }
            | Event::Reviewed { task, .. }
            | Event::EmergencyDeclared { task, .. } => Some(task),
            Event::Decided { task, .. } => task.as_ref(),
            Event::Intervened { what } => match what {
                Intervention::Stopped { task, .. } | Intervention::Replaced { task, .. } => {
                    Some(task)
                }
                _ => None,
            },
            Event::Refused { .. } | Event::Notified { .. } => None,
        }
    }

    /// Made from a status change, so the two cannot describe it differently.
    pub fn moved(task: &TaskId, from: Status, to: Status) -> Self {
        Event::Moved {
            task: task.clone(),
            from: from.to_string(),
            to: to.to_string(),
        }
    }
}

/// One line of the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub seq: u64,
    /// Unix seconds.
    pub at: u64,
    /// Who did it. An agent name, always.
    pub actor: String,
    #[serde(flatten)]
    pub event: Event,
}

/// The record itself.
/// An advisory exclusive lock on an open file, released when this drops.
///
/// `flock` is per open file description, and every append opens its own, so two threads in one
/// process contend exactly the same way two separate processes do. Advisory rather than
/// mandatory means it only holds against writers that also take it, which is every writer of
/// this file because they all go through `Journal::append`.
struct Lock<'a>(&'a std::fs::File);

impl<'a> Lock<'a> {
    fn exclusive(file: &'a std::fs::File) -> Result<Self> {
        use std::os::unix::io::AsRawFd;
        // Sound because the fd is owned by `file`, which outlives this guard by the lifetime,
        // so it cannot be closed while the lock is held.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self(file))
    }
}

impl Drop for Lock<'_> {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        // Sound for the same reason as above. Nothing useful can be done if it fails, and the
        // fd closing releases it regardless.
        unsafe {
            libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

/// The sequence number of the last whole record, without reading the whole file.
///
/// Reads a window off the end rather than everything, because this now runs on every append
/// rather than once at open. A record is a few hundred bytes, so the window holds many, and
/// falling back to a full read when no newline is found keeps it correct for a file smaller
/// than the window or one long line.
fn last_seq(path: &Path) -> Result<Option<u64>> {
    const WINDOW: u64 = 8 * 1024;

    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let len = file.metadata()?.len();
    if len == 0 {
        return Ok(None);
    }

    let from = len.saturating_sub(WINDOW);
    file.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);

    // Only whole lines. When the window starts mid line that first partial one is skipped,
    // unless the window covers the file from the start, where there is nothing to skip.
    let mut lines: Vec<&str> = text.lines().collect();
    if from > 0 && !lines.is_empty() {
        lines.remove(0);
    }

    let newest = lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .find_map(|l| serde_json::from_str::<Record>(l).ok().map(|r| r.seq));

    match newest {
        Some(seq) => Ok(Some(seq)),
        // No parsable record in the window, so fall back to the whole file rather than
        // restarting the numbering, which would hand out numbers already used.
        None => Ok(read(path)?.last().map(|r| r.seq)),
    }
}

pub struct Journal {
    path: PathBuf,
    next_seq: u64,
}

impl Journal {
    /// Opens the record, continuing the numbering where it left off.
    ///
    /// Reading the whole file to find the last sequence number is fine at this size and wrong
    /// at a million lines, and it is written down here so whoever hits that knows it was a
    /// choice.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }

        let next_seq = read(&path)?.last().map_or(1, |r| r.seq + 1);
        Ok(Self { path, next_seq })
    }

    /// Writes one line, and flushes it before returning.
    ///
    /// Flushed rather than buffered, because the whole value of this file is that it survives
    /// whatever happens next.
    ///
    /// Two things here exist because this file has more than one writer, and it is worth being
    /// clear that they are two separate problems with two separate fixes. The chain holds one
    /// `Journal` open for a whole run while the panel opens a fresh one per command, both
    /// against the same file. See bug 14.
    ///
    /// The sequence number comes from the file rather than from memory. A cached counter is
    /// only correct while one process is writing, and the moment a second one appends both
    /// hand out the same numbers. A duplicate `seq` makes a reader report a hole, and the
    /// reconnect then resumes past it, so a real record is lost for good.
    ///
    /// The line is built whole and written once. `writeln!` on a `File` emits the JSON and the
    /// newline as two separate syscalls, so a second writer landing between them produces one
    /// line holding two records followed by a blank one. `read` then discards that line and
    /// both records with it.
    pub fn append(&mut self, actor: &str, event: Event) -> Result<Record> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;

        // Held across reading the last number and writing the next one, because those two
        // steps are only correct together. Released when the guard drops, including on the
        // error paths below.
        let _guard = Lock::exclusive(&file)?;

        let seq = last_seq(&self.path)?.map_or(1, |s| s + 1);
        let record = Record {
            seq,
            at: now(),
            actor: actor.to_string(),
            event,
        };

        // Written through a shared reference, because the lock guard holds one for as long as
        // the lock is held. `Write` is implemented for `&File`, so this is the same syscall.
        let line = format!("{}\n", serde_json::to_string(&record)?);
        (&file).write_all(line.as_bytes())?;
        (&file).flush()?;

        self.next_seq = seq + 1;
        Ok(record)
    }

    pub fn path(&self) -> &Path {
        &self.path
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

/// Unix seconds. `pub(crate)` because the chain writes the same clock into agent folders, and
/// two clocks would let a journal entry and a state file disagree about when something happened.
pub(crate) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> (Journal, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let j = Journal::open(d.path().join("run/events.jsonl")).unwrap();
        (j, d)
    }

    /// The bug. `next_seq` was cached at open, so a long lived `Journal` and a short lived one
    /// writing to the same file both handed out the same numbers. And `writeln!` on a `File`
    /// emits the JSON and the newline as two syscalls, so a writer landing between them left
    /// one line holding two records followed by a blank one, which `read` then discards, losing
    /// both.
    ///
    /// The chain holds one journal open for a whole run while the panel opens a fresh one per
    /// command, against the same file, so this is the ordinary case rather than a stress test.
    #[test]
    fn concurrent_writers_do_not_collide_or_interleave() {
        const WRITERS: u64 = 4;
        const EACH: u64 = 150;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/events.jsonl");
        Journal::open(&path).unwrap();

        let done: Vec<_> = (0..WRITERS)
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    // Half hold one journal for the whole run, like the chain. Half reopen per
                    // write, like the panel. Both shapes are real and both are in this test.
                    let mut held = Journal::open(&path).unwrap();
                    for i in 0..EACH {
                        let event = Event::Decided {
                            task: None,
                            what: format!("w{w} i{i}"),
                        };
                        if w % 2 == 0 {
                            held.append("carl", event).unwrap();
                        } else {
                            Journal::open(&path).unwrap().append("carl", event).unwrap();
                        }
                    }
                })
            })
            .collect();
        for d in done {
            d.join().unwrap();
        }

        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let expected = (WRITERS * EACH) as usize;

        assert_eq!(
            lines.len(),
            expected,
            "expected {expected} lines, so records were glued together or lost"
        );
        assert!(
            lines.iter().all(|l| !l.trim().is_empty()),
            "a blank line means a record was written in two syscalls"
        );

        let records = read(&path).unwrap();
        assert_eq!(records.len(), expected, "a line failed to parse");

        let mut seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
        seqs.sort_unstable();
        let mut unique = seqs.clone();
        unique.dedup();
        assert_eq!(
            seqs.len(),
            unique.len(),
            "duplicate sequence numbers, which a reader reports as a hole and then resumes past"
        );
        assert_eq!(
            seqs,
            (1..=expected as u64).collect::<Vec<_>>(),
            "the sequence has to be every number once, with no gaps"
        );
    }

    /// The tail read has to give the same answer as reading everything, including when the
    /// window starts in the middle of a line.
    #[test]
    fn the_last_sequence_matches_a_full_read() {
        let (mut j, _d) = journal();
        for i in 0..200 {
            j.append(
                "carl",
                Event::Decided {
                    task: None,
                    what: format!("padding {i} {}", "x".repeat(200)),
                },
            )
            .unwrap();
        }
        let whole = read(j.path()).unwrap().last().unwrap().seq;
        assert_eq!(last_seq(j.path()).unwrap(), Some(whole));
    }

    #[test]
    fn the_last_sequence_of_a_missing_file_is_nothing() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(last_seq(&d.path().join("nope.jsonl")).unwrap(), None);
    }

    #[test]
    fn what_is_written_can_be_read_back() {
        let (mut j, _d) = journal();
        let task = TaskId::quoted("abc123");

        j.append(
            "mason",
            Event::Delegated {
                task: task.clone(),
                to: "nora".into(),
                goal: "fix the counter".into(),
                parent: None,
                must: vec!["it works".into()],
                project: None,
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].actor, "mason");
        assert_eq!(back[0].seq, 1);
        assert_eq!(back[0].event.kind(), "delegated");
    }

    #[test]
    fn numbering_continues_across_reopening() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("events.jsonl");

        let mut first = Journal::open(&path).unwrap();
        first
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "go".into(),
                },
            )
            .unwrap();
        drop(first);

        let mut second = Journal::open(&path).unwrap();
        let r = second
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "again".into(),
                },
            )
            .unwrap();

        assert_eq!(r.seq, 2, "a restart must not renumber from one");
    }

    /// The interesting half. Without it nobody can tell a rule that is working from a rule
    /// nothing has ever hit.
    #[test]
    fn refusals_are_recorded_too() {
        let (mut j, _d) = journal();
        j.append(
            "carl",
            Event::Refused {
                what: "delegate to nora".into(),
                why: "not a direct report".into(),
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back[0].event.kind(), "refused");
        assert_eq!(back[0].event.task(), None);
    }

    /// An exception nobody can count is an exception that becomes the habit.
    #[test]
    fn an_emergency_is_its_own_kind_of_event() {
        let (mut j, _d) = journal();
        let task = TaskId::quoted("t1");
        j.append(
            "mason",
            Event::EmergencyDeclared {
                task: task.clone(),
                why: "the build is broken and nora is not available".into(),
            },
        )
        .unwrap();

        let back = read(j.path()).unwrap();
        assert_eq!(back[0].event.kind(), "emergency_declared");
        assert_eq!(back[0].event.task(), Some(&task));
    }

    /// The two must not be able to describe the same change differently.
    #[test]
    fn a_move_is_built_from_the_statuses_themselves() {
        let e = Event::moved(&TaskId::quoted("t"), Status::Submitted, Status::Accepted);
        match e {
            Event::Moved { from, to, .. } => {
                assert_eq!(from, "submitted");
                assert_eq!(to, "accepted");
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn everything_about_one_task_can_be_pulled_out() {
        let (mut j, _d) = journal();
        let mine = TaskId::quoted("mine");
        let other = TaskId::quoted("other");

        j.append(
            "nora",
            Event::Submitted {
                task: mine.clone(),
                attempt: 1,
                words: 200,
            },
        )
        .unwrap();
        j.append(
            "nora",
            Event::Submitted {
                task: other.clone(),
                attempt: 1,
                words: 10,
            },
        )
        .unwrap();
        j.append(
            "mason",
            Event::Reviewed {
                task: mine.clone(),
                accepted: true,
                why: "good".into(),
            },
        )
        .unwrap();

        let story = about(&read(j.path()).unwrap(), &mine);
        assert_eq!(story.len(), 2);
        assert!(story.iter().all(|r| r.event.task() == Some(&mine)));
    }

    /// A record with one bad line is still worth reading, and refusing to open it would lose
    /// everything else.
    #[test]
    fn one_corrupt_line_does_not_lose_the_rest() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("events.jsonl");

        let mut j = Journal::open(&path).unwrap();
        j.append(
            "carl",
            Event::Decided {
                task: None,
                what: "one".into(),
            },
        )
        .unwrap();

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "this is not json at all").unwrap();
        drop(f);

        let mut j = Journal::open(&path).unwrap();
        j.append(
            "carl",
            Event::Decided {
                task: None,
                what: "two".into(),
            },
        )
        .unwrap();

        let back = read(&path).unwrap();
        assert_eq!(back.len(), 2, "the good lines survive: {back:?}");
    }

    #[test]
    fn a_record_that_does_not_exist_yet_reads_as_empty() {
        assert!(
            read("/definitely/not/here/events.jsonl")
                .unwrap()
                .is_empty()
        );
    }
}
