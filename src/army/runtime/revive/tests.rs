use super::*;
use crate::army::personnel::found;
use crate::army::runtime::Runtime;

fn home() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("a temp home");
    found(d.path(), 0).expect("found an army");
    d
}

/// Puts one agent into the state the supervisor leaves behind when it gives up.
fn give_up_on(home: &std::path::Path, name: &str, why: &str) {
    let people = Personnel::open(home).expect("personnel");
    let id = people
        .get(name)
        .and_then(|f| f.identity.as_ref())
        .map(|i| i.id.clone())
        .expect("an identity");

    let mut roll = Roll::open(home).expect("a roll");
    let mut record = Runtime::never(id, name, 0);
    record.lifecycle = Lifecycle::Degraded { why: why.into() };
    record.attempts = 6;
    roll.save(home, record).expect("save");
}

/// The whole point. A state only a human can leave needs a way for a human to leave it.
#[test]
fn a_given_up_agent_goes_back_into_the_ordinary_queue() {
    let d = home();
    give_up_on(d.path(), "miles", "6 starts in a row did not stick");

    let out = one(d.path(), "miles", false, 100).expect("revive");
    assert_eq!(
        out,
        Revived::Cleared {
            was: "6 starts in a row did not stick".into()
        }
    );

    let people = Personnel::open(d.path()).expect("personnel");
    let id = people
        .get("miles")
        .unwrap()
        .identity
        .as_ref()
        .unwrap()
        .id
        .clone();
    let roll = Roll::open(d.path()).expect("a roll");
    let after = roll.get(&id).expect("a record");
    assert!(
        matches!(after.lifecycle, Lifecycle::Exited { .. }),
        "{:?}",
        after.lifecycle
    );
    assert_eq!(after.attempts, 0, "the backoff counter was not cleared");
}

/// Reporting success for an agent that was already fine would teach somebody the command always
/// works, and then they would not read the answer.
#[test]
fn an_agent_that_was_not_given_up_on_is_told_so_rather_than_revived() {
    let d = home();
    assert_eq!(
        one(d.path(), "miles", false, 100).expect("revive"),
        Revived::NoRecord
    );
}

#[test]
fn a_running_agent_is_left_alone() {
    let d = home();
    let people = Personnel::open(d.path()).expect("personnel");
    let id = people
        .get("nora")
        .unwrap()
        .identity
        .as_ref()
        .unwrap()
        .id
        .clone();
    let mut roll = Roll::open(d.path()).expect("a roll");
    let mut record = Runtime::never(id, "nora", 0);
    record.lifecycle = Lifecycle::Running {
        pid: 1,
        started: 1,
        since: 0,
    };
    roll.save(d.path(), record).expect("save");

    match one(d.path(), "nora", false, 100).expect("revive") {
        Revived::NotGivenUp(why) => assert!(why.contains("running"), "{why}"),
        other => panic!("a running agent was touched: {other:?}"),
    }
}

/// Reviving is a thing that happened and belongs in the record, attributed to the person who
/// did it rather than to the supervisor, which did not decide it.
#[test]
fn reviving_is_written_down_as_jjs_act() {
    let d = home();
    give_up_on(d.path(), "miles", "six failed starts");
    let before = crate::army::survey::activity(d.path(), None, 1000)
        .expect("activity")
        .len();

    one(d.path(), "miles", false, 100).expect("revive");

    let after = crate::army::survey::activity(d.path(), None, 1000).expect("activity");
    assert_eq!(after.len(), before + 1, "nothing was recorded");
    let last = after.last().expect("a record");
    assert_eq!(
        last.actor, "jj",
        "the supervisor must not appear to have decided this"
    );
}

/// Twice is not an error and does not double anything, because somebody will run it twice.
#[test]
fn reviving_an_already_revived_agent_says_so_rather_than_failing() {
    let d = home();
    give_up_on(d.path(), "miles", "six failed starts");

    assert!(matches!(
        one(d.path(), "miles", false, 100).expect("first"),
        Revived::Cleared { .. }
    ));
    match one(d.path(), "miles", false, 101).expect("second") {
        Revived::NotGivenUp(why) => assert!(why.contains("ordinary queue"), "{why}"),
        other => panic!("a second revive did something: {other:?}"),
    }
}

#[test]
fn somebody_who_is_not_an_agent_is_refused_by_name() {
    let d = home();
    match one(d.path(), "hunter", false, 100) {
        Err(e) => assert!(e.to_string().contains("hunter"), "{e}"),
        Ok(other) => panic!("hunter is not an agent: {other:?}"),
    }
}

/// Reviving must not start anything. One way to start an agent, not two.
#[test]
fn reviving_never_starts_a_process() {
    let d = home();
    give_up_on(d.path(), "miles", "six failed starts");
    one(d.path(), "miles", false, 100).expect("revive");

    let people = Personnel::open(d.path()).expect("personnel");
    let id = people
        .get("miles")
        .unwrap()
        .identity
        .as_ref()
        .unwrap()
        .id
        .clone();
    let roll = Roll::open(d.path()).expect("a roll");
    assert!(
        !matches!(roll.get(&id).unwrap().lifecycle, Lifecycle::Running { .. }),
        "revive started a process itself"
    );
}

/// The loop this exists to break. A recorded session that names a conversation which no longer
/// exists makes every resume fail the same way, so reviving without clearing it just repeats.
#[test]
fn fresh_abandons_the_recorded_conversation() {
    let d = home();
    give_up_on(d.path(), "miles", "six failed starts");

    let people = Personnel::open(d.path()).expect("personnel");
    let id = people
        .get("miles")
        .unwrap()
        .identity
        .as_ref()
        .unwrap()
        .id
        .clone();
    let mut roll = Roll::open(d.path()).expect("a roll");
    let mut record = roll.get(&id).expect("a record").clone();
    let dead = crate::SessionId::fresh().expect("a session id");
    record.session = Some(dead.clone());
    roll.save(d.path(), record).expect("save");

    one(d.path(), "miles", true, 100).expect("revive");

    let roll = Roll::open(d.path()).expect("a roll");
    let after = roll.get(&id).expect("a record");
    assert!(after.session.is_none(), "the dead session was kept");
    assert!(
        after.abandoned.contains(&dead),
        "it was dropped instead of being kept as abandoned"
    );
}

/// The default keeps continuity, because a session that is fine is worth resuming and losing it
/// costs an agent its conversation for no reason.
#[test]
fn the_default_keeps_the_recorded_conversation() {
    let d = home();
    give_up_on(d.path(), "miles", "six failed starts");

    let people = Personnel::open(d.path()).expect("personnel");
    let id = people
        .get("miles")
        .unwrap()
        .identity
        .as_ref()
        .unwrap()
        .id
        .clone();
    let mut roll = Roll::open(d.path()).expect("a roll");
    let mut record = roll.get(&id).expect("a record").clone();
    record.session = Some(crate::SessionId::fresh().expect("a session id"));
    roll.save(d.path(), record).expect("save");

    one(d.path(), "miles", false, 100).expect("revive");

    let roll = Roll::open(d.path()).expect("a roll");
    assert!(
        roll.get(&id).expect("a record").session.is_some(),
        "reviving threw away a conversation nobody said was broken"
    );
}
