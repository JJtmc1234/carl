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
use crate::army::event::Event;
use crate::army::personnel::{Personnel, found};
use crate::army::runtime::Session as SessionContinuity;

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

/// Everything about one agent that was written to the journal, in order.
fn trail(home: &Path, agent: &crate::army::personnel::AgentId) -> Vec<String> {
    crate::army::event::read(home.join("run").join("events.jsonl"))
        .unwrap()
        .into_iter()
        .filter(|r| r.event.agent() == Some(agent))
        .map(|r| r.event.kind().to_string())
        .collect()
}

/// A record under `run/` answers what is true now. It cannot answer what happened, and "the
/// worker crashed and then the task was reported finished" is a sentence somebody has to be able
/// to read in order.
#[test]
fn starting_an_agent_is_written_down_with_what_survived_into_it() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();

    let records = crate::army::event::read(d.path().join("run/events.jsonl")).unwrap();
    let started: Vec<_> = records
        .iter()
        .filter(|r| r.event.kind() == "agent_started")
        .collect();
    assert_eq!(started.len(), 4, "carl, adrian, mason and nora");

    let hers = started
        .iter()
        .find(|r| r.event.agent() == Some(&nora))
        .unwrap();
    assert_eq!(hers.actor, "supervisor", "not nora, who did not do this");

    let Event::AgentStarted { continuity, .. } = &hers.event else {
        panic!("wrong event");
    };
    assert_eq!(continuity.process, Process::First);
    assert_eq!(continuity.session, SessionContinuity::Fresh);
    assert_eq!(continuity.memory, Memory::Kept, "she was given a folder");
    assert!(!continuity.degraded());
}

/// The whole point of separating the id from the session from the pid, written down where
/// somebody can check it afterwards rather than only implied by a record.
#[test]
fn a_process_replaced_under_a_kept_conversation_is_recorded_as_exactly_that() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();
    drop(sup);

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 2000).unwrap();

    assert_eq!(
        trail(d.path(), &nora),
        ["agent_started", "agent_crashed", "agent_started"],
        "started, went down with its supervisor, came back"
    );

    let latest = sup.roll().get(&nora).unwrap().continuity.unwrap();
    assert_eq!(latest.process, Process::Replaced);
    assert_eq!(latest.session, SessionContinuity::Resumed);
    assert!(
        !latest.degraded(),
        "a replaced process that resumed has lost nothing"
    );
}

/// An agent that comes back knowing nothing must not be reported as an ordinary restart, and
/// which conversation was given up on has to survive, because that transcript is the evidence.
#[test]
fn a_conversation_that_had_to_be_replaced_is_recorded_and_the_old_one_is_kept() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &falls_over());

    let mut now = 1000;
    for _ in 0..GIVE_UP_AFTER * 4 {
        let tick = sup.tick(&people, now).unwrap();
        settled(&sup);
        now = match tick.what.iter().find(|(n, _)| n == "nora").map(|(_, o)| o) {
            Some(Outcome::Waiting { until }) => *until,
            _ => now + 1,
        };
        if trail(d.path(), &nora).contains(&"continuity_changed".to_string()) {
            break;
        }
    }

    let records = crate::army::event::read(d.path().join("run/events.jsonl")).unwrap();
    let changed = records
        .iter()
        .find(|r| r.event.kind() == "continuity_changed" && r.event.agent() == Some(&nora))
        .expect("the conversation was set aside and nobody wrote it down");

    let Event::ContinuityChanged {
        from,
        to,
        abandoned,
        ..
    } = &changed.event
    else {
        panic!("wrong event");
    };
    assert_eq!(*from, SessionContinuity::Resumed);
    assert_eq!(*to, SessionContinuity::Replaced);

    let kept = abandoned
        .clone()
        .expect("which conversation was given up on");
    assert!(
        sup.roll().get(&nora).unwrap().abandoned.contains(&kept),
        "the record and the journal name the same one"
    );

    // Written once for a streak of failures, not once per attempt.
    let times = records
        .iter()
        .filter(|r| r.event.kind() == "continuity_changed" && r.event.agent() == Some(&nora))
        .count();
    assert_eq!(times, 1, "one session set aside per streak");
}

/// A start that produced no process is not a crash. A crash has a transcript to go and read and
/// this does not, and sending somebody looking for one wastes the only lead they had.
#[test]
fn a_start_that_never_produced_a_process_is_not_recorded_as_a_crash() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), Path::new("/nonexistent/definitely-not-claude"));

    sup.tick(&people, 1000).unwrap();

    let kinds = trail(d.path(), &nora);
    assert_eq!(kinds, ["agent_start_failed"]);
    assert!(!kinds.contains(&"agent_crashed".to_string()));
}

/// An agent nobody is trying to start looks exactly like an agent nobody has needed yet, and
/// those are not the same thing.
#[test]
fn giving_up_and_being_told_to_stop_are_two_different_records() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let nora = id_of(&people, "nora");

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();
    sup.stop(&nora, "JJ said so", 1001).unwrap();

    let kinds = trail(d.path(), &nora);
    assert_eq!(kinds, ["agent_started", "agent_stopped"]);

    let records = crate::army::event::read(d.path().join("run/events.jsonl")).unwrap();
    let stopped = records
        .iter()
        .rfind(|r| r.event.kind() == "agent_stopped")
        .unwrap();
    let Event::AgentStopped { why, .. } = &stopped.event else {
        panic!("wrong event");
    };
    assert_eq!(why, "JJ said so", "and the reason is kept");
}

/// A crash says nothing about whether the work succeeded, and the vocabulary must not let it.
#[test]
fn a_runtime_event_is_never_about_a_task() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();
    drop(sup);
    supervisor(d.path(), &stays_up())
        .tick(&people, 2000)
        .unwrap();

    let records = crate::army::event::read(d.path().join("run/events.jsonl")).unwrap();
    assert!(records.iter().any(|r| r.event.agent().is_some()));
    for record in records {
        if record.event.agent().is_some() {
            assert_eq!(
                record.event.task(),
                None,
                "{} claimed a task",
                record.event.kind()
            );
        }
    }
}

/// Two writers over one file, which is what the army actually is. The supervisor is recording
/// processes while Carl records work, and the order they went in has to survive.
#[test]
fn the_supervisor_and_carl_share_one_ordered_record() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let path = d.path().join("run").join("events.jsonl");

    let mut sup = supervisor(d.path(), &stays_up());
    sup.tick(&people, 1000).unwrap();

    let mut carl = crate::army::event::Journal::open(&path).unwrap();
    carl.append(
        "carl",
        Event::Decided {
            task: None,
            what: "the coding department takes this".into(),
        },
    )
    .unwrap();

    sup.tick(&people, 1001).unwrap();
    let nora = id_of(&people, "nora");
    sup.stop(&nora, "enough for today", 1002).unwrap();

    let records = crate::army::event::read(&path).unwrap();
    let seqs: Vec<u64> = records.iter().map(|r| r.seq).collect();
    assert_eq!(
        seqs,
        (1..=records.len() as u64).collect::<Vec<_>>(),
        "one place in the order each, and none reused"
    );

    let kinds: Vec<&str> = records.iter().map(|r| r.event.kind()).collect();
    let decided = kinds.iter().position(|k| *k == "decided").unwrap();
    let stopped = kinds.iter().position(|k| *k == "agent_stopped").unwrap();
    assert!(decided < stopped, "and the order is the order it happened");
}

/// A local hour, turned back into a timestamp this machine agrees is that hour.
///
/// The tests below need "three in the morning" and "midday" as unix seconds, and the machine's
/// zone decides which seconds those are. Searching for it rather than computing it keeps the
/// zone table out of the test, which is the same reason `local_hour` exists.
fn at_local_hour(wanted: u32) -> u64 {
    let start = 1_787_000_000;
    for step in 0..48 {
        let when = start + step * 3600;
        if crate::army::personnel::local_hour(when) == Some(wanted) {
            return when;
        }
    }
    panic!("no timestamp in two days had local hour {wanted}");
}

/// An army whose every agent keeps the same window, so a test can pick an hour and know what
/// the whole army should do at it. Founding gives the chief no window at all, which is correct
/// and makes for a poor fixture.
fn all_sleeping(dir: &Path, hours: crate::army::personnel::Hours) -> Personnel {
    found(dir, 100).unwrap();
    for name in ["carl", "adrian", "mason", "nora"] {
        let path = dir.join("army").join(name).join("config.json");
        let mut config: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        config["hours"] = serde_json::to_value(hours).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).unwrap();
    }
    Personnel::open(dir).unwrap()
}

#[test]
fn an_army_inside_its_window_is_put_down_and_stays_down() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let mut sup = supervisor(d.path(), &stays_up());
    let night = at_local_hour(3);

    sup.tick(&people, night).unwrap();
    assert_eq!(sup.holding(), 4, "up before the timetable is consulted");

    let changed = sup.keep_hours(&people, night).unwrap();
    assert_eq!(changed.len(), 4, "all four were put down");
    assert_eq!(sup.holding(), 0, "and their processes let go");

    // And the tick that follows leaves them alone rather than starting them again, which is the
    // whole point of the state being its own rather than an absence.
    let tick = sup.tick(&people, night).unwrap();
    assert_eq!(
        tick.count(|o| matches!(o, Outcome::NotStarting { .. })),
        4,
        "{:?}",
        tick.lines()
    );
    assert_eq!(sup.holding(), 0);
}

#[test]
fn a_second_pass_inside_the_window_changes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let mut sup = supervisor(d.path(), &stays_up());
    let night = at_local_hour(2);

    sup.keep_hours(&people, night).unwrap();
    let again = sup.keep_hours(&people, night + 60).unwrap();
    assert!(again.is_empty(), "nothing changed, so nothing is reported");
}

/// The morning. The agent comes back on the conversation it had, because sleeping is a process
/// ending and a process ending is the one thing this whole layer is built to survive.
#[test]
fn the_window_ending_lets_an_agent_start_again_on_the_same_conversation() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());

    let night = at_local_hour(3);
    sup.tick(&people, night).unwrap();
    let before = sup.roll().get(&nora).unwrap().session.clone();
    sup.keep_hours(&people, night).unwrap();

    let morning = at_local_hour(9);
    let changed = sup.keep_hours(&people, morning).unwrap();
    assert_eq!(changed.len(), 4, "all four are allowed up again");

    let tick = sup.tick(&people, morning).unwrap();
    assert_eq!(
        tick.count(|o| matches!(o, Outcome::Started(Start::Resume))),
        4,
        "resumed rather than started over: {:?}",
        tick.lines()
    );
    assert_eq!(sup.roll().get(&nora).unwrap().session, before);
    assert!(before.is_some(), "and there was a conversation to keep");
}

/// Sleeping must not count as failing to start, or an army would degrade itself over a week of
/// perfectly ordinary nights.
#[test]
fn a_night_costs_an_agent_no_attempts_and_no_backoff() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());

    let night = at_local_hour(1);
    sup.tick(&people, night).unwrap();
    sup.keep_hours(&people, night).unwrap();
    sup.keep_hours(&people, at_local_hour(8)).unwrap();

    let record = sup.roll().get(&nora).unwrap();
    assert_eq!(record.attempts, 0, "a night is not a failure");
    // Started at once rather than sat in a backoff worked out from a fresh timestamp.
    let tick = sup.tick(&people, at_local_hour(8)).unwrap();
    assert_eq!(tick.count(|o| matches!(o, Outcome::Started(_))), 4);
}

/// The collision the design note flagged first. Somebody needs an agent at two in the morning,
/// and the timetable must not take it away again on the next pass.
#[test]
fn an_agent_woken_inside_its_window_is_left_up_until_the_window_ends() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());

    let night = at_local_hour(1);
    sup.tick(&people, night).unwrap();
    sup.keep_hours(&people, night).unwrap();

    let woke = sup
        .wake(
            &nora,
            crate::army::event::Because::Incident {
                what: "the panel stopped answering".into(),
            },
            night + 60,
        )
        .unwrap();
    assert!(woke, "she was asleep, so waking her did something");

    // The timetable runs again, still inside the window, and leaves her alone.
    let changed = sup.keep_hours(&people, night + 120).unwrap();
    assert!(
        !changed.iter().any(|(name, _)| name == "nora"),
        "nora was put back to sleep: {changed:?}"
    );
    let tick = sup.tick(&people, night + 120).unwrap();
    assert_eq!(
        tick.what
            .iter()
            .find(|(n, _)| n == "nora")
            .map(|(_, o)| o.clone()),
        Some(Outcome::Started(Start::Resume)),
        "and she is started rather than refused"
    );
}

/// Being woken exempts an agent from the rest of one window, not from every window after it.
#[test]
fn an_agent_woken_one_night_still_sleeps_the_next() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());

    let night = at_local_hour(1);
    sup.keep_hours(&people, night).unwrap();
    sup.wake(
        &nora,
        crate::army::event::Because::Lead {
            who: "mason".into(),
        },
        night + 60,
    )
    .unwrap();

    // Morning clears it, without her having to be asleep at the time.
    sup.keep_hours(&people, at_local_hour(10)).unwrap();
    assert!(!sup.roll().get(&nora).unwrap().roused);

    // And the next night she goes down like everybody else.
    let changed = sup.keep_hours(&people, at_local_hour(23)).unwrap();
    assert!(
        changed.iter().any(|(name, _)| name == "nora"),
        "nora stayed up a second night: {changed:?}"
    );
}

/// The failure the state exists to prevent. An agent JJ switched off must not come back by
/// itself in the morning.
#[test]
fn the_morning_does_not_undo_a_decision_to_stop() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");
    let mut sup = supervisor(d.path(), &stays_up());

    sup.tick(&people, at_local_hour(12)).unwrap();
    sup.stop(&nora, "JJ wanted her off", at_local_hour(12))
        .unwrap();

    sup.keep_hours(&people, at_local_hour(2)).unwrap();
    sup.keep_hours(&people, at_local_hour(9)).unwrap();

    let record = sup.roll().get(&nora).unwrap();
    assert!(
        matches!(&record.lifecycle, Lifecycle::Stopped { why } if why.contains("JJ")),
        "{:?}",
        record.lifecycle
    );
}

/// An agent the supervisor gave up on is the thing a person is meant to look at, so the night
/// must not quietly file it away as asleep.
#[test]
fn a_degraded_agent_is_not_put_to_sleep_over_it() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");

    // Written before the supervisor opens, which is the only way in from outside and is exactly
    // how a supervisor that gave up last night hands the state to the one running now.
    let mut roll = Roll::open(d.path()).unwrap();
    let mut record = Runtime::never(nora.clone(), "nora", 1);
    record.lifecycle = Lifecycle::Degraded {
        why: "will not start".into(),
    };
    roll.save(d.path(), record).unwrap();
    drop(roll);

    let mut sup = supervisor(d.path(), &stays_up());
    sup.keep_hours(&people, at_local_hour(3)).unwrap();
    assert!(matches!(
        sup.roll().get(&nora).unwrap().lifecycle,
        Lifecycle::Degraded { .. }
    ));
}

/// The chief keeps no hours, so nothing about the night touches him.
#[test]
fn an_agent_with_no_window_is_never_put_down() {
    let d = tempfile::tempdir().unwrap();
    let people = army(d.path());
    let mut sup = supervisor(d.path(), &stays_up());
    let night = at_local_hour(3);

    sup.tick(&people, night).unwrap();
    let changed = sup.keep_hours(&people, night).unwrap();

    assert!(
        !changed.iter().any(|(name, _)| name == "carl"),
        "carl has no window and was put down anyway: {changed:?}"
    );
    assert_eq!(changed.len(), 3, "the other three do have one");
}

/// Sleeping is a thing that happened to an agent, and the record has to be able to tell it
/// apart from somebody deciding to stop one.
#[test]
fn going_to_sleep_is_recorded_as_its_own_event() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let mut sup = supervisor(d.path(), &stays_up());

    sup.keep_hours(&people, at_local_hour(3)).unwrap();

    let records = crate::army::event::read(people.journal_path()).unwrap();
    let slept: Vec<_> = records
        .iter()
        .filter(|r| r.event.kind() == "agent_slept")
        .collect();
    assert_eq!(slept.len(), 4);
    assert!(
        !records.iter().any(|r| r.event.kind() == "agent_stopped"),
        "a night is not somebody deciding to stop an agent"
    );
}

/// A process left behind by a supervisor that has gone is still a model sitting there billing.
/// Putting the agent down for the night has to end it, and this supervisor holds no pipe to it,
/// so it takes the same route a reclaim does.
///
/// The orphan is built rather than produced by killing a supervisor, because two supervisors
/// cannot share a home and inside one test process there is only one pid to be. A record naming
/// a live process and somebody else's supervisor is exactly the state either way.
#[test]
fn sleeping_ends_a_process_left_by_a_supervisor_that_is_gone() {
    let d = tempfile::tempdir().unwrap();
    let people = all_sleeping(d.path(), crate::army::personnel::Hours::night());
    let nora = id_of(&people, "nora");

    let mut orphan = std::process::Command::new(stays_up())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("the stand in runs");
    let pid = orphan.id();
    let started = crate::providers::system::started::started(pid).expect("it is running");

    let mut record = Runtime::never(nora.clone(), "nora", 1);
    record.lifecycle = Lifecycle::Running {
        pid,
        started,
        since: 1,
    };
    record.session = Some(crate::SessionId::fresh().unwrap());
    // Somebody else's supervisor, which is what makes this an orphan rather than ours.
    record.supervisor = Some(u32::MAX);
    let mut roll = Roll::open(d.path()).unwrap();
    roll.save(d.path(), record).unwrap();
    drop(roll);

    let mut sup = supervisor(d.path(), &stays_up());
    sup.keep_hours(&people, at_local_hour(3)).unwrap();

    assert!(
        !crate::providers::system::started::is_still(pid, started),
        "the orphaned process was left running all night"
    );
    assert!(matches!(
        sup.roll().get(&nora).unwrap().lifecycle,
        Lifecycle::Asleep { .. }
    ));

    // Reaped here rather than left for the harness, so the test leaves nothing behind.
    let _ = orphan.wait();
}
