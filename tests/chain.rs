//! The whole chain, walked with the real types and no model anywhere.
//!
//! The unit tests check each rule on its own. This checks that the rules together can actually
//! express the flow JJ specified, which is a different question and the one that matters before
//! two other processes build on them. A set of types that each behave correctly and cannot be
//! composed into the intended shape is a design fault, and it is much cheaper to find here than
//! after somebody has written a delegation engine against it.
//!
//! Nothing here calls Claude. Every step is a type moving, which is why it runs in
//! milliseconds and can be run on every change.

use carl::army::event::{self, Event, Journal};
use carl::army::task::{MAX_ATTEMPTS, may_take_on};
use carl::army::{Status, Task, Verification};

fn verification() -> Verification {
    Verification::of([
        "cargo test passes in the worktree",
        "the new test fails without the change",
    ])
    .unwrap()
}

/// JJ wants something, and it reaches Nora through everybody in between.
#[test]
fn work_reaches_the_worker_through_every_link() {
    let from_jj = Task::assign("jj", "carl", "make JJtorio start faster", verification()).unwrap();
    let to_adrian = Task::split_from(
        &from_jj,
        "carl",
        "adrian",
        "the coding side of it",
        verification(),
    )
    .unwrap();
    let to_mason = Task::split_from(
        &to_adrian,
        "adrian",
        "mason",
        "the Factorio side of it",
        verification(),
    )
    .unwrap();
    let to_nora = Task::split_from(
        &to_mason,
        "mason",
        "nora",
        "cache the prototype lookup",
        verification(),
    )
    .unwrap();

    assert_eq!(to_nora.owner, "nora");
    assert_eq!(to_nora.created_by, "mason");
    assert_eq!(to_nora.parent.as_ref(), Some(&to_mason.id));

    // The line back up is walkable, which is what makes a tree of work possible later.
    assert_eq!(to_mason.parent.as_ref(), Some(&to_adrian.id));
    assert_eq!(to_adrian.parent.as_ref(), Some(&from_jj.id));
    assert!(from_jj.parent.is_none());
}

/// Every shortcut somebody would reach for on a busy afternoon.
#[test]
fn no_link_in_the_chain_can_be_skipped() {
    for (from, to) in [
        ("carl", "mason"),
        ("carl", "nora"),
        ("adrian", "nora"),
        ("jj", "nora"),
        ("jj", "adrian"),
    ] {
        let attempt = Task::assign(from, to, "just this once", verification());
        assert!(
            attempt.is_err(),
            "{from} should not be able to hand work straight to {to}"
        );
    }
}

/// The ordinary life of a task, first time.
#[test]
fn a_task_done_properly_the_first_time() {
    let mut t = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();

    assert!(may_take_on("nora", std::slice::from_ref(&t)).is_ok());
    t.advance("nora", Status::InHand).unwrap();
    t.advance("nora", Status::Submitted).unwrap();
    t.advance("mason", Status::Accepted).unwrap();

    assert!(t.settled());
    assert_eq!(t.attempts, 1);
    assert!(!t.must_escalate());
}

/// The governance rule, walked the way it will actually happen.
#[test]
fn three_rejections_and_it_goes_over_masons_head() {
    let mut t = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();

    for attempt in 1..=MAX_ATTEMPTS {
        t.advance("nora", Status::InHand).unwrap();
        t.advance("nora", Status::Submitted).unwrap();
        assert_eq!(t.attempts, attempt);
        assert!(!t.must_escalate(), "not judged yet on attempt {attempt}");

        t.advance("mason", Status::ChangesRequested).unwrap();
    }

    assert!(t.must_escalate());
    assert_eq!(t.attempts_left(), 0);

    // Escalating is not a state change. Mason takes it to Adrian, which is a new task with the
    // failed one as its parent, so the history of the original stays true.
    let upward = Task::split_from(
        &t,
        "adrian",
        "mason",
        "nora has been round three times on this, take it from here",
        verification(),
    )
    .unwrap();
    assert_eq!(upward.parent.as_ref(), Some(&t.id));
    assert_eq!(
        t.status,
        Status::ChangesRequested,
        "the original is untouched"
    );
}

/// One at a time, all the way through, which is the rule as somebody would actually hit it.
#[test]
fn nora_cannot_be_given_a_second_task_while_working() {
    let mut first = Task::assign("mason", "nora", "first thing", verification()).unwrap();
    first.advance("nora", Status::InHand).unwrap();

    // Creating the second is allowed. Handing it to her while she is working is not, and that
    // is the distinction: a lead may line work up and may not pile it on.
    let second = Task::assign("mason", "nora", "second thing", verification()).unwrap();
    let held = [first.clone(), second.clone()];
    assert!(may_take_on("nora", &held).is_err());

    first.advance("nora", Status::Submitted).unwrap();
    first.advance("mason", Status::Accepted).unwrap();

    let held = [first, second];
    assert!(
        may_take_on("nora", &held).is_ok(),
        "once the first is approved she is free"
    );
}

/// The record has to be able to answer the questions a hierarchy exists to make answerable.
#[test]
fn the_record_can_say_who_did_what_to_which_task() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(dir.path().join("run/events.jsonl")).unwrap();

    let mut t = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
            },
        )
        .unwrap();

    let before = t.status;
    t.advance("nora", Status::InHand).unwrap();
    journal
        .append("nora", Event::moved(&t.id, before, t.status))
        .unwrap();

    t.advance("nora", Status::Submitted).unwrap();
    journal
        .append(
            "nora",
            Event::Submitted {
                task: t.id.clone(),
                attempt: t.attempts,
                words: 420,
            },
        )
        .unwrap();

    t.advance("mason", Status::Accepted).unwrap();
    journal
        .append(
            "mason",
            Event::Reviewed {
                task: t.id.clone(),
                accepted: true,
                why: "the test fails without it".into(),
            },
        )
        .unwrap();

    let story = event::about(&event::read(journal.path()).unwrap(), &t.id);
    assert_eq!(story.len(), 4);

    // Who reviewed it, which is the question the whole hierarchy exists to make answerable.
    let reviewer = story
        .iter()
        .find(|r| r.event.kind() == "reviewed")
        .expect("no review recorded");
    assert_eq!(reviewer.actor, "mason");
    assert_ne!(reviewer.actor, t.owner, "nobody reviews their own work");

    // And in order, which is what makes it a history rather than a pile.
    let seqs: Vec<u64> = story.iter().map(|r| r.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "{seqs:?}");
}

/// A refusal is worth as much as an action, and this is the shape somebody would record one in.
#[test]
fn a_refused_shortcut_is_recorded_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut journal = Journal::open(dir.path().join("events.jsonl")).unwrap();

    let refused = Task::assign("carl", "nora", "just do it", verification()).unwrap_err();
    journal
        .append(
            "carl",
            Event::Refused {
                what: "assign work to nora".into(),
                why: refused.to_string(),
            },
        )
        .unwrap();

    let back = event::read(journal.path()).unwrap();
    assert_eq!(back[0].event.kind(), "refused");

    match &back[0].event {
        Event::Refused { why, .. } => {
            assert!(why.contains("mason"), "the record says the route: {why}")
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// Carl may never implement, and a lead only under something recorded.
#[test]
fn who_may_actually_write_code() {
    use carl::army::check_may_implement;

    assert!(check_may_implement("nora", false).is_ok());

    assert!(check_may_implement("mason", false).is_err());
    assert!(check_may_implement("mason", true).is_ok());
    assert!(check_may_implement("adrian", true).is_ok());

    assert!(check_may_implement("carl", false).is_err());
    assert!(
        check_may_implement("carl", true).is_err(),
        "no emergency makes the chief executive an implementer"
    );
}
