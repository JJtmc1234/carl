//! Finding what is stuck, and moving exactly that.

use super::*;
use crate::army::task::{Task, Verification};

fn verification() -> Verification {
    Verification::of(["the tests pass"]).unwrap()
}

fn board() -> (tempfile::TempDir, Board, ()) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.jsonl");
    let board = Board::at(&path).unwrap();
    (dir, board, ())
}

/// A lead holding work with nothing under it is the shape of a stalled chain.
#[test]
fn a_lead_sitting_on_work_is_what_a_pass_looks_for() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("carl", "adrian", "get the issues down", verification()).unwrap();
    board.delegate("carl", &held).unwrap();

    let stuck = waiting_on_a_lead(&board).unwrap();
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0].owner, "adrian");
}

/// A worker holding work is not stuck, it is working, and moving it would be wrong.
#[test]
fn a_worker_holding_work_is_left_alone() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    board.delegate("mason", &held).unwrap();

    assert!(
        waiting_on_a_lead(&board).unwrap().is_empty(),
        "a working agent was treated as a stalled chain"
    );
}

/// Once a lead has handed on, it is waiting rather than sitting, and a second pass must not
/// hand the same work down twice.
#[test]
fn a_lead_that_has_already_handed_on_is_not_stuck_again() {
    let (_d, mut board, _journal) = board();
    let held = Task::assign("carl", "adrian", "get the issues down", verification()).unwrap();
    board.delegate("carl", &held).unwrap();

    let said = "AGENT:\niris\n\nTASK:\ntriage the open issues\n\nDONE WHEN:\n- each names a file\n";
    let (agent, _task) = hand_on_one(&mut board, None, "adrian", &held, said).unwrap();
    assert_eq!(agent, "iris");

    assert!(
        waiting_on_a_lead(&board).unwrap().is_empty(),
        "the same work would have been handed down twice"
    );
}

/// Finished work is history and must not be reopened by a pass.
#[test]
fn settled_work_is_not_picked_up() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("carl", "adrian", "done long ago", verification()).unwrap();
    board.delegate("carl", &held).unwrap();
    board
        .advance("adrian", &held.id, crate::army::task::Status::InHand)
        .unwrap();
    board.submit("adrian", &held.id, 10).unwrap();
    board.review("carl", &held.id, true, "good").unwrap();

    assert!(waiting_on_a_lead(&board).unwrap().is_empty());
}

/// The rule is the same here as everywhere: a lead cannot reach past its own people, whatever
/// the answer said.
#[test]
fn a_pass_refuses_an_answer_that_names_somebody_elses_agent() {
    let (_d, mut board, _journal) = board();
    let held = Task::assign("carl", "adrian", "get the issues down", verification()).unwrap();
    board.delegate("carl", &held).unwrap();

    let said = "AGENT:\nnora\n\nTASK:\ndo the factorio thing\n\nDONE WHEN:\n- it works\n";
    let err = hand_on_one(&mut board, None, "adrian", &held, said)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot hand work to"), "{err}");

    assert_eq!(
        board.tasks().unwrap().len(),
        1,
        "a refused hand on still created a task"
    );
}

/// What the board holds and what the journal holds must not come apart. A task in one and not
/// the other is a task the panel cannot see or a task the record cannot explain.
#[test]
fn a_handed_on_task_is_on_the_board_and_in_the_record() {
    let (dir, mut board, _journal) = board();
    let held = Task::assign("carl", "olivia", "keep the mail moving", verification()).unwrap();
    board.delegate("carl", &held).unwrap();

    let said =
        "AGENT:\nmiles\n\nTASK:\ntriage the inbox\n\nDONE WHEN:\n- nothing important is missed\n";
    let (agent, task) = hand_on_one(&mut board, None, "olivia", &held, said).unwrap();
    assert_eq!(agent, "miles");

    let on_board = board.tasks().unwrap();
    assert!(
        on_board
            .iter()
            .any(|t| t.id == task.id && t.owner == "miles")
    );

    let records = crate::army::event::read(dir.path().join("events.jsonl")).unwrap();
    assert!(
        records.iter().any(|r| r.actor == "olivia"),
        "the record does not say olivia did anything"
    );
}

/// The handover is recorded once, by the board and nobody else.
///
/// It was recorded twice at first: `assign::hand_on` appended a `Delegated` and then
/// `board.delegate` appended another. The board reads the journal to rebuild the tasks, found
/// the id already delegated, and refused the very handover it had just been given. Two writers
/// for one fact do not merely duplicate it, they disagree.
#[test]
fn a_handover_is_written_exactly_once() {
    let (dir, mut board, _journal) = board();
    let held = Task::assign("carl", "adrian", "get the issues down", verification()).unwrap();
    board.delegate("carl", &held).unwrap();

    let said = "AGENT:\niris\n\nTASK:\ntriage the open issues\n\nDONE WHEN:\n- each names a file\n";
    let (_agent, task) = hand_on_one(&mut board, None, "adrian", &held, said).unwrap();

    let records = crate::army::event::read(dir.path().join("events.jsonl")).unwrap();
    let for_this: Vec<_> = records
        .iter()
        .filter(|r| {
            matches!(&r.event, crate::army::event::Event::Delegated { task: t, .. } if *t == task.id)
        })
        .collect();
    assert_eq!(
        for_this.len(),
        1,
        "the handover was recorded {} times",
        for_this.len()
    );
}

/// Work comes back up by being reviewed, and until now only the way down existed.
#[test]
fn submitted_work_is_what_a_review_pass_looks_for() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    board.delegate("mason", &held).unwrap();
    board
        .advance("nora", &held.id, crate::army::task::Status::InHand)
        .unwrap();

    assert!(
        waiting_on_review(&board).unwrap().is_empty(),
        "work in hand is not waiting on anybody"
    );

    board.submit("nora", &held.id, 40).unwrap();
    let waiting = waiting_on_review(&board).unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(
        waiting[0].created_by, "mason",
        "the reviewer is whoever asked for it"
    );
}

/// Accepting is not the default when a verdict cannot be read.
///
/// A reviewer who says something unreadable has not approved anything, and treating that as
/// approval is how work nobody checked gets marked done.
#[test]
fn an_unreadable_verdict_is_not_an_acceptance() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    board.delegate("mason", &held).unwrap();
    board
        .advance("nora", &held.id, crate::army::task::Status::InHand)
        .unwrap();
    board.submit("nora", &held.id, 40).unwrap();

    let (accepted, _why) = review_one(&mut board, None, &held, "hmm, not sure really").unwrap();
    assert!(!accepted, "unreadable was taken as approval");

    let after = board.get(&held.id).unwrap().unwrap();
    assert_eq!(after.status, crate::army::task::Status::ChangesRequested);
}

/// A clear acceptance finishes the task.
#[test]
fn an_accepted_task_is_accepted() {
    let (_d, mut board, _j) = board();
    let held = Task::assign("olivia", "miles", "triage the inbox", verification()).unwrap();
    board.delegate("olivia", &held).unwrap();
    board
        .advance("miles", &held.id, crate::army::task::Status::InHand)
        .unwrap();
    board.submit("miles", &held.id, 20).unwrap();

    let (accepted, why) = review_one(
        &mut board,
        None,
        &held,
        "Accept. Nothing important was missed.",
    )
    .unwrap();
    assert!(
        accepted,
        "a clear acceptance was read as a rejection: {why}"
    );
    assert_eq!(
        board.get(&held.id).unwrap().unwrap().status,
        crate::army::task::Status::Accepted
    );
    assert!(waiting_on_review(&board).unwrap().is_empty());
}
