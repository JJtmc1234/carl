//! The properties a board has to have, and the two that are the whole reason it exists.
//!
//! A task completes exactly once, and a restart does not create a second owner. Everything else
//! here is the ordinary machinery that makes those two true.

use super::*;
use crate::army::task::Verification;

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn board(at: &tempfile::TempDir) -> Board {
    Board::open(at.path()).unwrap()
}

/// One real task down one real step of the chain.
fn task_for(created_by: &str, owner: &str) -> Task {
    Task::assign(
        created_by,
        owner,
        "write result.txt in the task workspace",
        Verification::of(["result.txt exists and says done"]).unwrap(),
    )
    .unwrap()
}

/// A task handed to Nora, on the board, in hand.
fn in_hand(b: &mut Board) -> TaskId {
    let task = task_for("mason", "nora");
    b.delegate("mason", &task).unwrap();
    b.advance("nora", &task.id, Status::InHand).unwrap();
    task.id
}

#[test]
fn a_task_handed_down_can_be_read_back_whole() {
    let d = home();
    let mut b = board(&d);
    let task = task_for("mason", "nora");
    b.delegate("mason", &task).unwrap();

    let back = b.get(&task.id).unwrap().expect("it is on the board");
    assert_eq!(back.goal, task.goal);
    assert_eq!(back.owner, "nora");
    assert_eq!(back.created_by, "mason");
    assert_eq!(back.status, Status::Assigned);
    assert_eq!(back.verification.must, task.verification.must);
}

/// The record is the only copy, so a board that has never seen the task before has to be able
/// to answer everything about it. This is what a restarted Carl does.
#[test]
fn a_new_board_over_the_same_record_sees_the_same_work() {
    let d = home();
    let id = {
        let mut first = board(&d);
        let id = in_hand(&mut first);
        first.submit("nora", &id, 12).unwrap();
        id
    };

    let second = board(&d);
    let task = second.get(&id).unwrap().unwrap();
    assert_eq!(task.status, Status::Submitted);
    assert_eq!(task.owner, "nora", "and the same owner, not a new one");
    assert_eq!(task.attempts, 1);
}

/// The property. Two boards, both correct about what they read, and the task finishes once.
#[test]
fn a_task_can_only_be_accepted_once_however_many_boards_are_looking() {
    let d = home();
    let mut mason = board(&d);
    let id = in_hand(&mut mason);
    mason.submit("nora", &id, 12).unwrap();

    // A second process that rebuilt the same task and reached the same conclusion.
    let mut also = board(&d);

    mason
        .review("mason", &id, true, "checked, it is there")
        .unwrap();
    let again = also.review("mason", &id, true, "checked, it is there");

    let e = again.unwrap_err().to_string();
    assert!(e.contains("cannot become"), "{e}");
    assert_eq!(also.get(&id).unwrap().unwrap().status, Status::Accepted);

    let accepted = also
        .records()
        .unwrap()
        .into_iter()
        .filter(|r| r.event.kind() == "reviewed")
        .count();
    assert_eq!(accepted, 1, "one review in the record, not two");
}

/// The same under contention, because a race that only shows up when the timing is unlucky is
/// the one that reaches production.
#[test]
fn many_boards_racing_to_accept_produce_exactly_one_acceptance() {
    let d = home();
    let mut mason = board(&d);
    let id = in_hand(&mut mason);
    mason.submit("nora", &id, 12).unwrap();
    drop(mason);

    let won: usize = std::thread::scope(|scope| {
        let hands: Vec<_> = (0..8)
            .map(|_| {
                let path = d.path().to_path_buf();
                let id = id.clone();
                scope.spawn(move || {
                    let mut b = Board::open(&path).unwrap();
                    b.review("mason", &id, true, "checked").is_ok()
                })
            })
            .collect();
        hands
            .into_iter()
            .filter_map(|h| h.join().ok())
            .filter(|won| *won)
            .count()
    });

    assert_eq!(won, 1, "exactly one accepted it");
    let b = board(&d);
    assert_eq!(b.get(&id).unwrap().unwrap().status, Status::Accepted);
    assert_eq!(
        b.records()
            .unwrap()
            .iter()
            .filter(|r| r.event.kind() == "reviewed")
            .count(),
        1
    );
}

/// The single most tempting shortcut in the whole design, refused at the record rather than at
/// a value somebody is holding.
#[test]
fn a_worker_cannot_accept_her_own_work() {
    let d = home();
    let mut b = board(&d);
    let id = in_hand(&mut b);
    b.submit("nora", &id, 12).unwrap();

    let e = b
        .review("nora", &id, true, "looks right to me")
        .unwrap_err();
    assert!(e.to_string().contains("cannot move this task"), "{e}");
    assert_eq!(b.get(&id).unwrap().unwrap().status, Status::Submitted);
}

/// Submitting is the worker saying so. Accepting is somebody else having checked. A design
/// where the first implies the second has no review in it at all.
#[test]
fn submitting_is_not_finishing() {
    let d = home();
    let mut b = board(&d);
    let id = in_hand(&mut b);
    let task = b.submit("nora", &id, 40).unwrap();

    assert_eq!(task.status, Status::Submitted);
    assert!(!task.status.settled(), "nothing has been decided yet");
}

/// A task nobody could get on with keeps its owner. Putting it back would let somebody else
/// start it, and then two agents are doing one task and the second does not know.
#[test]
fn a_blocked_task_keeps_its_owner_and_can_be_picked_back_up() {
    let d = home();
    let mut b = board(&d);
    let id = in_hand(&mut b);

    let blocked = b.advance("nora", &id, Status::Blocked).unwrap();
    assert_eq!(blocked.status, Status::Blocked);
    assert_eq!(blocked.owner, "nora", "still hers");
    assert!(!blocked.status.settled());

    let back = b.advance("nora", &id, Status::InHand).unwrap();
    assert_eq!(back.status, Status::InHand);
}

/// A task nobody could get on with is not a task that got done while nobody was looking.
#[test]
fn a_blocked_task_cannot_go_straight_to_review() {
    let d = home();
    let mut b = board(&d);
    let id = in_hand(&mut b);
    b.advance("nora", &id, Status::Blocked).unwrap();

    let e = b.submit("nora", &id, 10).unwrap_err().to_string();
    assert!(e.contains("cannot become"), "{e}");
}

/// The chain is enforced when the task is built. The board must not be a second door into the
/// same room.
#[test]
fn work_cannot_be_handed_to_somebody_who_does_not_report_to_you() {
    assert!(
        Task::assign("carl", "nora", "do it", Verification::of(["done"]).unwrap()).is_err(),
        "carl reaching past adrian and mason"
    );

    let d = home();
    let mut b = board(&d);
    let task = task_for("mason", "nora");
    let e = b.delegate("adrian", &task).unwrap_err().to_string();
    assert!(e.contains("cannot hand down"), "{e}");
}

/// A worker with a longer list is a worker choosing what to do, which is its lead's job.
#[test]
fn a_worker_may_be_given_a_primary_and_three_backups_and_no_more() {
    let d = home();
    let mut b = board(&d);

    for n in 0..AT_ONCE {
        let task = task_for("mason", "nora");
        b.delegate("mason", &task)
            .unwrap_or_else(|e| panic!("task {n}: {e}"));
    }
    assert_eq!(b.holding("nora").unwrap().len(), AT_ONCE);

    let one_too_many = task_for("mason", "nora");
    let e = b.delegate("mason", &one_too_many).unwrap_err().to_string();
    assert!(e.contains("as many as anybody may have"), "{e}");
}

/// Finished work stops counting, otherwise a worker fills up permanently after four tasks.
#[test]
fn a_finished_task_makes_room_for_another() {
    let d = home();
    let mut b = board(&d);

    let mut ids = Vec::new();
    for _ in 0..AT_ONCE {
        let task = task_for("mason", "nora");
        b.delegate("mason", &task).unwrap();
        ids.push(task.id);
    }
    assert!(b.delegate("mason", &task_for("mason", "nora")).is_err());

    b.advance("nora", &ids[0], Status::InHand).unwrap();
    b.submit("nora", &ids[0], 5).unwrap();
    b.review("mason", &ids[0], true, "fine").unwrap();

    assert_eq!(b.holding("nora").unwrap().len(), AT_ONCE - 1);
    assert!(b.delegate("mason", &task_for("mason", "nora")).is_ok());
}

/// The same task handed down twice is one task. Rebuilding it from the second line would reset
/// the status of work that has since moved on.
#[test]
fn a_repeated_handover_does_not_reopen_a_task() {
    let d = home();
    let mut b = board(&d);
    let task = task_for("mason", "nora");
    b.delegate("mason", &task).unwrap();
    b.advance("nora", &task.id, Status::InHand).unwrap();

    assert!(b.delegate("mason", &task).is_err(), "already handed down");
    assert_eq!(b.get(&task.id).unwrap().unwrap().status, Status::InHand);
}

/// A task with no goal is a task nobody could review, so a line that cannot rebuild one is
/// skipped rather than turned into an empty task somebody then accepts.
#[test]
fn a_handover_with_nothing_to_check_is_not_rebuilt_into_a_task() {
    let d = home();
    let mut journal = crate::army::event::Journal::open(d.path().join("run/events.jsonl")).unwrap();
    let id = TaskId::quoted("no-conditions");
    journal
        .append(
            "mason",
            Event::Delegated {
                task: id.clone(),
                to: "nora".into(),
                goal: "something".into(),
                parent: None,
                must: Vec::new(),
                project: None,
            },
        )
        .unwrap();

    assert!(board(&d).get(&id).unwrap().is_none());
}

/// Nothing may be done to a task the record has never heard of, however confidently it is
/// named. A quoted id is a claim and not a fact.
#[test]
fn a_task_that_was_never_handed_down_cannot_be_moved() {
    let d = home();
    let mut b = board(&d);
    let e = b
        .advance("nora", &TaskId::quoted("invented"), Status::InHand)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no task"), "{e}");
}
