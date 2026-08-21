//! The supervisor against real processes, because the interesting half is not the decision.
//!
//! `policy` proves what should happen. This proves what does, and the two are different in
//! exactly the places that matter: a process that exits the moment it starts, a record that
//! outlives the supervisor that wrote it, a pid that is alive and unreachable.
//!
//! The stand in is a shell script rather than `claude`, for the reasons `tests/session.rs`
//! already gives: a model is slow, costs money and never does the same thing twice. What is
//! being tested here is process lifetime, and a script that sleeps has exactly the lifetime a
//! test asks it to.

use std::path::{Path, PathBuf};

use super::*;
use crate::army::personnel::{Personnel, found};

/// A stand in that reads its stdin until it is closed, which is what a held open session does.
fn stays_up() -> PathBuf {
    stand_in("agent-stays-up")
}

/// A stand in that exits immediately, which is what a broken one does.
fn falls_over() -> PathBuf {
    stand_in("agent-falls-over")
}

/// One of the checked in stand ins under `tests/stand-in/`.
///
/// Checked in rather than written here, and the scripts themselves say why. In short: a test
/// that writes an executable and then runs it races every other thread in this binary, and the
/// usual answer of retrying the spawn would mean retrying a supervisor tick, which counts an
/// attempt and changes the thing under test.
fn stand_in(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("stand-in")
        .join(name);
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

/// A founded home and its people, which is what a supervisor is pointed at.
fn army(dir: &Path) -> Personnel {
    found(dir, 100).unwrap();
    Personnel::open(dir).unwrap()
}

fn supervisor(home: &Path, program: &Path) -> Supervisor {
    Supervisor::take(home, program).expect("the home is a fresh temporary directory")
}

fn id_of(people: &Personnel, name: &str) -> crate::army::personnel::AgentId {
    people.identity(name).unwrap().id.clone()
}

/// Waits until nothing the supervisor started is still running.
///
/// A tick spawns a process and returns. A process that is going to fall over has not necessarily
/// fallen over yet, so a test that ticks in a tight loop is racing the shell it just started, and
/// counts one attempt where it meant to count several. Waiting for the exit is the difference
/// between a test about the restart policy and a test about scheduling luck.
fn settled(sup: &Supervisor) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        let running = sup.roll().all().any(|r| match r.lifecycle {
            Lifecycle::Running { pid, started, .. } => {
                crate::providers::system::started::is_still(pid, started)
            }
            _ => false,
        });
        if !running {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("something the supervisor started never exited");
}

#[test]
fn a_first_tick_starts_every_agent_in_a_new_conversation() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let mut sup = supervisor(d.path(), &stays_up());

    let tick = sup.tick(&people, 1000).unwrap();
    assert_eq!(tick.what.len(), 4, "carl, adrian, mason and nora");
    assert_eq!(
        tick.count(|o| matches!(o, Outcome::Started(Start::Fresh))),
        4
    );
    assert_eq!(sup.holding(), 4);
}

#[test]
fn a_second_tick_leaves_running_agents_alone_rather_than_starting_more() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let mut sup = supervisor(d.path(), &stays_up());

    sup.tick(&people, 1000).unwrap();
    let again = sup.tick(&people, 1001).unwrap();

    assert_eq!(again.count(|o| matches!(o, Outcome::Left)), 4);
    assert_eq!(sup.holding(), 4, "and no second process for anybody");
}

/// The property the whole layer exists for. The process is disposable, the conversation is not,
/// and the agent is neither.
#[test]
fn a_replaced_process_keeps_the_same_agent_and_the_same_conversation() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();
    let first = sup.roll().get(&nora).unwrap().clone();
    let first_pid = first.lifecycle.pid().unwrap();

    // The supervisor goes away, taking its processes with it, exactly as a restart would.
    drop(sup);

    let mut sup = supervisor(d.path(), &stays_up());
    let tick = sup.tick(&people, 2000).unwrap();
    let second = sup.roll().get(&nora).unwrap().clone();

    assert_eq!(
        tick.what
            .iter()
            .find(|(n, _)| n == "nora")
            .map(|(_, o)| o.clone()),
        Some(Outcome::Started(Start::Resume)),
        "resumed rather than started over"
    );
    assert_eq!(second.agent, first.agent, "the same agent");
    assert_eq!(second.session, first.session, "the same conversation");
    assert_ne!(
        second.lifecycle.pid().unwrap(),
        first_pid,
        "and a different process"
    );
}

/// A process that will not stay up is counted, backed off and eventually given up on, and none
/// of that may happen silently.
#[test]
fn an_agent_that_will_not_stay_up_is_backed_off_and_then_given_up_on() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &falls_over());

    // The clock moves the way a real supervisor's does: a second at a time while there is
    // something to look at, and straight to the deadline when the only answer is to wait. That
    // matters more than it looks. Advancing by a fixed large step instead would mean every exit
    // was observed long after the start, and so counted as a process that had stayed up.
    let mut now = 1000;
    let mut ticks = 0;
    while ticks < GIVE_UP_AFTER * 4 {
        let tick = sup.tick(&people, now).unwrap();
        settled(&sup);
        ticks += 1;

        now = match tick.what.iter().find(|(n, _)| n == "nora").map(|(_, o)| o) {
            Some(Outcome::Waiting { until }) => *until,
            _ => now + 1,
        };
        if matches!(
            sup.roll().get(&nora).map(|r| &r.lifecycle),
            Some(Lifecycle::Degraded { .. })
        ) {
            break;
        }
    }

    let record = sup.roll().get(&nora).unwrap();
    assert!(
        matches!(record.lifecycle, Lifecycle::Degraded { .. }),
        "it never gave up: {:?}",
        record.lifecycle
    );
    assert!(
        ticks >= GIVE_UP_AFTER,
        "it gave up after {ticks} passes, without really trying"
    );
    assert!(
        !record.abandoned.is_empty(),
        "and the session it gave up on was kept, not deleted"
    );
}

/// Backoff is the difference between one broken agent and a busy loop that makes the machine
/// worse, so the wait has to be real rather than a decision nobody acts on.
#[test]
fn an_agent_in_backoff_is_not_started_again_on_the_next_pass() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let mut sup = supervisor(d.path(), &falls_over());

    sup.tick(&people, 1000).unwrap();
    settled(&sup);
    // The first exit is free, so this pass counts it and starts again straight away.
    sup.tick(&people, 1001).unwrap();
    settled(&sup);
    let third = sup.tick(&people, 1002).unwrap();

    assert_eq!(
        third.count(|o| matches!(o, Outcome::Waiting { .. })),
        4,
        "every one of them is waiting rather than being hammered"
    );
}

/// A stopped agent is a decision, and the supervisor's job is to respect it rather than to
/// notice a gap and helpfully fill it.
#[test]
fn a_stopped_agent_is_not_started_and_the_reason_survives_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();
    sup.stop(&nora, "JJ wanted the room quiet", 1100).unwrap();
    drop(sup);

    let mut sup = supervisor(d.path(), &stays_up());
    let tick = sup.tick(&people, 2000).unwrap();
    let (_, outcome) = tick.what.iter().find(|(n, _)| n == "nora").unwrap();

    let Outcome::NotStarting { why } = outcome else {
        panic!("nora should not have been started: {outcome:?}");
    };
    assert!(why.contains("quiet"), "{why}");
    assert_eq!(
        tick.count(|o| matches!(o, Outcome::Started(_) | Outcome::Left)),
        3,
        "and the other three are untouched"
    );
}

/// An agent with no id has nothing durable for a process to belong to. Starting one anyway
/// would produce a process nobody could find again.
#[test]
fn an_agent_without_an_identity_is_skipped_and_says_why() {
    let d = tempfile::tempdir().unwrap();
    found(d.path(), 100).unwrap();
    std::fs::remove_file(d.path().join("army").join("nora").join("identity.json")).unwrap();
    let people = Personnel::open(d.path()).unwrap();

    let mut sup = supervisor(d.path(), &stays_up());
    let tick = sup.tick(&people, 1000).unwrap();
    let (_, outcome) = tick.what.iter().find(|(n, _)| n == "nora").unwrap();

    assert!(
        matches!(outcome, Outcome::Skipped { why } if why.contains("identity")),
        "{outcome:?}"
    );
    assert_eq!(sup.holding(), 3, "and nothing was started for her");
}

/// A supervisor that cannot spawn at all must record that and count it, not carry on believing
/// the agent is running.
#[test]
fn a_claude_that_cannot_be_run_is_recorded_as_a_failed_start() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");

    let mut sup = supervisor(d.path(), &d.path().join("no-such-binary"));
    let tick = sup.tick(&people, 1000).unwrap();

    assert_eq!(tick.count(|o| matches!(o, Outcome::Failed { .. })), 4);
    let record = sup.roll().get(&nora).unwrap();
    assert!(matches!(record.lifecycle, Lifecycle::Exited { .. }));
    assert_eq!(record.attempts, 1, "counted once, here rather than twice");
    assert_eq!(sup.holding(), 0);
}

/// Two supervisors on one home would each read the other's records, decide its processes were
/// orphans and end them, all night, both behaving exactly as designed.
#[test]
fn a_second_supervisor_cannot_take_a_home_that_is_already_claimed() {
    let d = tempfile::tempdir().unwrap();
    let program = stays_up();
    let _first = supervisor(d.path(), &program);

    let err = match Supervisor::take(d.path(), &program) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("a second supervisor took the same home"),
    };
    assert!(err.contains("already running"), "{err}");
}

/// The supervisor owns process existence and nothing else. There is deliberately no way from
/// here to hand an agent a task, and this is what would notice one appearing.
#[test]
fn the_supervisor_has_no_way_to_give_an_agent_work() {
    let source = include_str!("supervisor.rs");
    for word in ["Task", "task::", "delegate", "Delegated"] {
        assert!(
            !source.contains(word),
            "{word} should not appear in the supervisor: work is Carl's"
        );
    }
}

/// An agent's process runs in its own folder, so what it can reach by default is its own memory
/// rather than whatever directory the supervisor happened to be started in.
#[test]
fn an_agent_is_started_in_its_own_folder() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    assert!(people.memory_dir("nora").starts_with(people.folder("nora")));

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();

    let pid = sup
        .roll()
        .get(&id_of(&people, "nora"))
        .unwrap()
        .lifecycle
        .pid()
        .unwrap();
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).unwrap();
    assert_eq!(
        cwd.canonicalize().unwrap(),
        people.folder("nora").canonicalize().unwrap()
    );
}
