//! What the record must hold, and what must never happen to the numbering.
//!
//! Split from the vocabulary and the file handling because they are two small modules and one
//! set of tests that crosses both. A test that writes an event and reads it back is about both
//! halves at once, and putting it in either one would be a choice about which half it belonged
//! to that nobody could defend.

use std::io::Write;

use super::*;
use crate::army::task::Status;

fn journal() -> (Journal, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let j = Journal::open(d.path().join("run/events.jsonl")).unwrap();
    (j, d)
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
            workspace: None,
            objective: None,
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

/// Two writers over one file, which is a supervisor and Carl in the same home.
///
/// Without the catch up inside the lock, both of these hold `next_seq` from the moment they were
/// opened and both hand out 1, 2, 3. The file then has two different events claiming the same
/// place in the order, which is the single thing a sequence number exists to rule out.
#[test]
fn two_journals_over_one_file_never_reuse_a_sequence_number() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("run/events.jsonl");

    let mut supervisor = Journal::open(&path).unwrap();
    let mut carl = Journal::open(&path).unwrap();

    for round in 0..4 {
        supervisor
            .append(
                "supervisor",
                Event::Refused {
                    what: format!("round {round}"),
                    why: "test".into(),
                },
            )
            .unwrap();
        carl.append(
            "carl",
            Event::Decided {
                task: None,
                what: format!("round {round}"),
            },
        )
        .unwrap();
    }

    let seqs: Vec<u64> = read(&path).unwrap().iter().map(|r| r.seq).collect();
    assert_eq!(seqs, (1..=8).collect::<Vec<_>>());
}

/// The same thing under contention, because a lock that is only taken when nobody wants it is
/// not a lock. Every line has to be there, and every number has to be used once.
///
/// Threads rather than processes, and it is the same test either way: `flock` is held per open
/// file, not per process, so eight threads that each opened the file contend exactly as eight
/// processes would. Spawning processes would only make it slower and harder to read.
#[test]
fn many_writers_at_once_still_number_every_line_exactly_once() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("run/events.jsonl");
    Journal::open(&path).unwrap();

    let writers = 8;
    let each = 20;
    std::thread::scope(|scope| {
        for who in 0..writers {
            let path = path.clone();
            scope.spawn(move || {
                let mut journal = Journal::open(&path).unwrap();
                for n in 0..each {
                    journal
                        .append(
                            "supervisor",
                            Event::Refused {
                                what: format!("{who}-{n}"),
                                why: "test".into(),
                            },
                        )
                        .unwrap();
                }
            });
        }
    });

    let records = read(&path).unwrap();
    assert_eq!(records.len(), writers * each, "no line was lost");

    let mut seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
    seqs.sort_unstable();
    assert_eq!(
        seqs,
        (1..=(writers * each) as u64).collect::<Vec<_>>(),
        "every number used once and none skipped"
    );
}

/// A journal that has written nothing yet still has to notice what is already there, otherwise
/// the first line it writes lands on top of somebody else's numbering.
#[test]
fn a_journal_opened_before_another_wrote_catches_up_on_its_first_append() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("events.jsonl");

    let mut early = Journal::open(&path).unwrap();
    let mut other = Journal::open(&path).unwrap();
    for _ in 0..3 {
        other
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "something".into(),
                },
            )
            .unwrap();
    }

    let written = early
        .append(
            "supervisor",
            Event::Refused {
                what: "late".into(),
                why: "test".into(),
            },
        )
        .unwrap();
    assert_eq!(written.seq, 4, "not 1, which is where it left off");
}

/// A file that got shorter is a file something replaced, and the honest answer is to keep
/// going rather than to go back.
///
/// Tempting to reread from the start and take the numbering from what is left. That would hand
/// out a number this journal has already used, and a crash caught mid write leaves exactly this
/// shape, so a reader who saw the old line and the new one would see two events at seq 3. The
/// numbering only ever moves forward.
#[test]
fn a_replaced_file_does_not_make_the_numbering_go_backwards() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("events.jsonl");

    let mut journal = Journal::open(&path).unwrap();
    for _ in 0..5 {
        journal
            .append(
                "carl",
                Event::Decided {
                    task: None,
                    what: "a fairly long line so the file is definitely shorter afterwards".into(),
                },
            )
            .unwrap();
    }

    let two = read(&path).unwrap()[1].clone();
    std::fs::write(&path, format!("{}\n", serde_json::to_string(&two).unwrap())).unwrap();

    let written = journal
        .append(
            "carl",
            Event::Decided {
                task: None,
                what: "after".into(),
            },
        )
        .unwrap();
    assert_eq!(written.seq, 6, "still after everything this journal wrote");
}
