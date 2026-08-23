//! One objective from Carl to a worker and back, against the real machinery.
//!
//! ```text
//!   jj        asks for something, and is the only authority that is not delegated
//!    -> carl      turns it into an objective and decides whose department it is
//!        -> mason     writes the concrete task, grants the workspace, reviews what comes back
//!            -> nora      does exactly that and reports
//! ```
//!
//! Four actors rather than three, because that is what the organisation actually is. Factorio
//! used to hang under engineering, which put a second lead in this path. It is its own
//! department now, so the depth below Carl is one lead, and the fourth actor is JJ at the top
//! where he always was. Carl still cannot hand work to Nora however short the path looks.
//! Building a shorter version would have meant inventing an organisation to test against, and
//! then the test would prove something about the invention.
//!
//! **No model anywhere in here.** The processes are the real ones the supervisor starts, and
//! what they run is a checked in shell script that obeys one instruction. A test that called a
//! model would be slow, cost money and never do the same thing twice, and none of the three
//! things being proved here are about what a model says.
//!
//! What is real: the supervisor, the process lifetimes, the journal and its numbering, the
//! board, every rule in `org.rs` and `task.rs`, and the agent folders on disk. What is standing
//! in: the four agents' judgement. The test decides what each of them decides, which is what
//! makes it deterministic and is exactly the part a model would supply.
//!
//! The three properties everything else here exists to support:
//!
//! - an agent dying never means its task succeeded
//! - a task completes exactly once, however many times anything is restarted
//! - identity survives a process, a conversation, and both at once

use std::path::{Path, PathBuf};
use std::time::Duration;

use carl::army::board::Board;
use carl::army::event::{Because, Event, Record};
use carl::army::personnel::{AgentId, Personnel, found};
use carl::army::runtime::{Lifecycle, Session as SessionContinuity, Supervisor};
use carl::army::task::{Status, Task, TaskId, Verification};

/// How long a stand in gets to answer. Generous for a shell script, and finite.
const PATIENCE: Duration = Duration::from_secs(10);

fn stand_in(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("stand-in")
        .join(name);
    assert!(path.is_file(), "{} is missing", path.display());
    path
}

/// The army, its record, and the one supervisor over it.
struct Slice {
    home: tempfile::TempDir,
    people: Personnel,
    board: Board,
    supervisor: Supervisor,
}

impl Slice {
    fn founded(program: &Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        found(home.path(), 100).unwrap();
        Self {
            people: Personnel::open(home.path()).unwrap(),
            board: Board::open(home.path()).unwrap(),
            supervisor: Supervisor::take(home.path(), program).unwrap(),
            home,
        }
    }

    fn id(&self, name: &str) -> AgentId {
        self.people.identity(name).unwrap().id.clone()
    }

    fn lifecycle(&self, name: &str) -> Lifecycle {
        self.supervisor
            .roll()
            .get(&self.id(name))
            .map(|r| r.lifecycle.clone())
            .unwrap_or(Lifecycle::Never)
    }

    fn records(&self) -> Vec<Record> {
        carl::army::event::read(self.home.path().join("run/events.jsonl")).unwrap()
    }

    /// Everything the record says about one agent's process, in order.
    fn runtime_trail(&self, name: &str) -> Vec<String> {
        let id = self.id(name);
        self.records()
            .into_iter()
            .filter(|r| r.event.agent() == Some(&id))
            .map(|r| r.event.kind().to_string())
            .collect()
    }

    /// Waits until the process this agent's record names has actually gone.
    ///
    /// One agent rather than all of them. A tick spawns and returns, so a process that is about
    /// to die has not necessarily died yet, and a test that carried on would be racing the shell
    /// it just killed. Waiting for everybody instead would hang, because the other three are
    /// meant to still be up.
    fn gone(&self, name: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let running = match self.lifecycle(name) {
                Lifecycle::Running { pid, started, .. } => {
                    carl::providers::system::started::is_still(pid, started)
                }
                _ => false,
            };
            if !running {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{name}'s process never exited");
    }
}

/// One task down one step of the chain, with something a reviewer can actually check.
fn task(created_by: &str, owner: &str, goal: &str, must: &str) -> Task {
    Task::assign(created_by, owner, goal, Verification::of([must]).unwrap()).unwrap()
}

/// The full path, in the order the army walks it.
#[test]
fn an_objective_goes_from_carl_to_a_worker_and_an_accepted_result_comes_back() {
    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));

    // 1. Carl decides the worker is not needed yet. A permanent agent asleep is the ordinary
    //    state overnight, and it is the supervisor that carries it out rather than decides it.
    slice.supervisor.tick(&slice.people, 1_000).unwrap();
    slice
        .supervisor
        .stop(&slice.id("nora"), "nothing for her yet", 1_001)
        .unwrap();
    assert!(matches!(slice.lifecycle("nora"), Lifecycle::Stopped { .. }));

    let tick = slice.supervisor.tick(&slice.people, 1_002).unwrap();
    assert_eq!(
        tick.count(|o| matches!(o, carl::army::runtime::Outcome::NotStarting { .. })),
        1,
        "only the sleeping one is left alone"
    );

    // 2. JJ asks Carl for something. This is the only step in the whole walk that starts
    //    outside the army.
    let objective = task(
        "jj",
        "carl",
        "the army can show it wrote something it was asked for",
        "a result file exists where the worker was allowed to write",
    );
    slice.board.delegate("jj", &objective).unwrap();
    slice
        .board
        .advance("carl", &objective.id, Status::InHand)
        .unwrap();

    // 3. Carl decides it is Mason's department, and splits it rather than passing it on whole.
    //    He cannot reach past Mason to Nora and there is no way here to try.
    let departmental = Task::split_from(
        &objective,
        "carl",
        "mason",
        "have the factorio worker produce the result file",
        Verification::of(["result.txt is there and says done"]).unwrap(),
    )
    .unwrap();
    slice.board.delegate("carl", &departmental).unwrap();
    slice
        .board
        .advance("mason", &departmental.id, Status::InHand)
        .unwrap();

    // 4. Mason writes the concrete task and says where it may happen. The grant is recorded
    //    here. What enforces it is the capability layer the worker runs against.
    let workspace = slice.people.folder("nora").join("work");
    std::fs::create_dir_all(&workspace).unwrap();

    let concrete = Task::split_from(
        &departmental,
        "mason",
        "nora",
        "write result.txt in the workspace you were given",
        Verification::of(["result.txt exists in the granted workspace and says done"]).unwrap(),
    )
    .unwrap()
    .in_workspace(workspace.display().to_string());

    slice.board.delegate("mason", &concrete).unwrap();
    slice
        .board
        .grant(
            "mason",
            &concrete.id,
            &format!("write under {}", workspace.display()),
        )
        .unwrap();

    // 5. Mason needs her, so he asks for her, and the asking names what for.
    slice
        .supervisor
        .wake(
            &slice.id("nora"),
            Because::Task {
                task: concrete.id.clone(),
            },
            1_003,
        )
        .unwrap();
    // She was asleep, so this really did wake her.
    assert!(matches!(slice.lifecycle("nora"), Lifecycle::Exited { .. }));
    slice.supervisor.tick(&slice.people, 1_004).unwrap();
    assert!(matches!(slice.lifecycle("nora"), Lifecycle::Running { .. }));

    // 6. She picks it up and does it. The instruction travels through the supervisor because the
    //    supervisor holds the pipe, and the supervisor did not write a word of it.
    slice
        .board
        .advance("nora", &concrete.id, Status::InHand)
        .unwrap();

    let target = workspace.join("result.txt");
    let said = slice
        .supervisor
        .deliver(
            &slice.id("nora"),
            &format!("WRITE {}", target.display()),
            PATIENCE,
        )
        .unwrap();
    assert!(said.contains("wrote"), "{said}");

    // 7. She says it is done. Saying so is not finishing.
    let submitted = slice
        .board
        .submit("nora", &concrete.id, said.len())
        .unwrap();
    assert_eq!(submitted.status, Status::Submitted);
    assert_eq!(submitted.attempts, 1);

    // 8. Mason checks rather than trusts, by looking at the thing itself.
    let on_disk = std::fs::read_to_string(&target).expect("the worker's actual output");
    assert_eq!(on_disk.trim(), "done");
    assert!(
        target.starts_with(&workspace),
        "and it landed inside what was granted"
    );

    let accepted = slice
        .board
        .review("mason", &concrete.id, true, "read the file, it says done")
        .unwrap();
    assert_eq!(accepted.status, Status::Accepted);

    // 9. Back up, one step at a time. Nobody skips a level going up either, because a task is
    //    only reviewable by whoever created it.
    slice.board.submit("mason", &departmental.id, 20).unwrap();
    slice
        .board
        .review("carl", &departmental.id, true, "checked the worker's file")
        .unwrap();

    slice.board.submit("carl", &objective.id, 20).unwrap();
    slice
        .board
        .review(
            "jj",
            &objective.id,
            true,
            "the department did what was asked",
        )
        .unwrap();

    // 10. Carl records where the objective got to.
    let mut journal =
        carl::army::event::Journal::open(slice.home.path().join("run/events.jsonl")).unwrap();
    journal
        .append(
            "carl",
            Event::Decided {
                task: Some(objective.id.clone()),
                what: "the army produced what it was asked for".into(),
            },
        )
        .unwrap();

    // Everything finished, once each.
    for id in [&concrete.id, &departmental.id, &objective.id] {
        let task = slice.board.get(id).unwrap().unwrap();
        assert_eq!(task.status, Status::Accepted, "{id}");
    }
    let reviews = slice
        .records()
        .iter()
        .filter(|r| r.event.kind() == "reviewed")
        .count();
    assert_eq!(reviews, 3, "one review per task and no more");

    // And the same record answers whether the army is getting better, without a second file
    // anybody had to remember to write. This is the measure the flagship is judged on, taken
    // off the only walk in this repository that uses real processes.
    let measured = carl::army::metrics::of(&slice.records());
    assert_eq!(
        measured.objectives.len(),
        1,
        "one objective, not three tasks"
    );
    assert_eq!(measured.objectives[0].goal, objective.goal);
    assert!(measured.objectives[0].accepted());
    assert!(
        measured.objectives[0].unattended(),
        "JJ opened it and never had to come back"
    );
    assert_eq!(measured.reviews.accepted, 3);
    assert_eq!(measured.reviews.rejected, 0);
    assert_eq!(measured.retries.repeats, 0, "nobody had to do it twice");
    assert_eq!(measured.recovery.crashes, 0);
    assert_eq!(measured.interventions_each(), Some(0.0));

    // The record reads as one ordered story rather than as two files that have to be lined up.
    let seqs: Vec<u64> = slice.records().iter().map(|r| r.seq).collect();
    assert_eq!(
        seqs,
        (1..=seqs.len() as u64).collect::<Vec<_>>(),
        "one place in the order each"
    );

    let kinds: Vec<String> = slice
        .records()
        .iter()
        .map(|r| r.event.kind().to_string())
        .collect();
    for expected in [
        "agent_started",
        "agent_stopped",
        "delegated",
        "granted",
        "agent_woken",
        "moved",
        "submitted",
        "reviewed",
        "decided",
    ] {
        assert!(
            kinds.contains(&expected.to_string()),
            "no {expected}: {kinds:?}"
        );
    }

    // First occurrence for the ones that happen once, and last for `decided`, because founding
    // the army decides things too and those lines are already in the file.
    let at = |kind: &str| kinds.iter().position(|k| k == kind).unwrap();
    let last = |kind: &str| kinds.iter().rposition(|k| k == kind).unwrap();
    assert!(
        at("delegated") < at("granted"),
        "granted before there was work"
    );
    assert!(
        at("granted") < at("agent_woken"),
        "woken before being allowed"
    );
    assert!(
        at("submitted") < at("reviewed"),
        "reviewed before it was offered"
    );
    assert!(
        last("reviewed") < last("decided"),
        "decided before it was reviewed"
    );

    // 11. And everything stops.
    let holding = slice.supervisor.holding();
    assert!(holding > 0, "there were processes to stop");
    drop(slice.supervisor);
}

/// The property that matters most, and the one a runtime is most likely to get wrong.
#[test]
fn killing_the_worker_never_finishes_its_task() {
    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));
    slice.supervisor.tick(&slice.people, 1_000).unwrap();

    let lead_task = task("mason", "nora", "write the file", "result.txt says done");
    slice.board.delegate("mason", &lead_task).unwrap();
    slice
        .board
        .advance("nora", &lead_task.id, Status::InHand)
        .unwrap();

    let before = slice
        .supervisor
        .roll()
        .get(&slice.id("nora"))
        .unwrap()
        .clone();
    let pid = before.lifecycle.pid().expect("she has a process");

    // Killed outright, which is the shape of a crash that leaves nothing behind.
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) }, 0);
    slice.gone("nora");

    // The supervisor notices, writes it down, and starts her again in the same conversation.
    slice.supervisor.tick(&slice.people, 1_001).unwrap();

    assert_eq!(
        slice.runtime_trail("nora"),
        ["agent_started", "agent_crashed", "agent_started"],
        "started, crashed, started again"
    );

    let after = slice
        .supervisor
        .roll()
        .get(&slice.id("nora"))
        .unwrap()
        .clone();
    assert_eq!(after.agent, before.agent, "the same agent");
    assert_eq!(after.session, before.session, "the same conversation");
    assert_ne!(after.lifecycle.pid().unwrap(), pid, "a different process");
    assert_eq!(
        after.continuity.unwrap().session,
        SessionContinuity::Resumed,
        "and it was resumed rather than replaced"
    );

    // The task did not move, and nothing about a dead process could have moved it.
    let task = slice.board.get(&lead_task.id).unwrap().unwrap();
    assert_eq!(task.status, Status::InHand, "still in hand");
    assert_eq!(task.owner, "nora", "still hers, not back in a queue");
    assert!(!task.status.settled());

    assert!(
        slice
            .records()
            .iter()
            .filter(|r| r.event.agent().is_some())
            .all(|r| r.event.task().is_none()),
        "a runtime event claimed a task"
    );
}

/// Once the crash is over, the work finishes once and is reviewed once, whatever happened in
/// the middle.
#[test]
fn work_interrupted_by_a_crash_still_completes_exactly_once() {
    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));
    slice.supervisor.tick(&slice.people, 1_000).unwrap();

    let workspace = slice.people.folder("nora").join("work");
    std::fs::create_dir_all(&workspace).unwrap();
    let assigned = task("mason", "nora", "write the file", "result.txt says done")
        .in_workspace(workspace.display().to_string());
    slice.board.delegate("mason", &assigned).unwrap();
    slice
        .board
        .advance("nora", &assigned.id, Status::InHand)
        .unwrap();

    let pid = slice
        .supervisor
        .roll()
        .get(&slice.id("nora"))
        .unwrap()
        .lifecycle
        .pid()
        .unwrap();
    assert_eq!(unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) }, 0);
    slice.gone("nora");

    // Her lead is the one who decides what a dead worker means for the work. The supervisor,
    // which is the only thing that knows she died, has no way to touch a task.
    slice
        .board
        .advance("nora", &assigned.id, Status::Blocked)
        .unwrap();
    assert_eq!(
        slice.board.get(&assigned.id).unwrap().unwrap().owner,
        "nora",
        "blocked, not handed to somebody else"
    );

    slice.supervisor.tick(&slice.people, 1_001).unwrap();
    slice
        .board
        .advance("nora", &assigned.id, Status::InHand)
        .unwrap();

    let target = workspace.join("result.txt");
    slice
        .supervisor
        .deliver(
            &slice.id("nora"),
            &format!("WRITE {}", target.display()),
            PATIENCE,
        )
        .unwrap();
    slice.board.submit("nora", &assigned.id, 10).unwrap();

    assert_eq!(
        std::fs::read_to_string(&target).unwrap().trim(),
        "done",
        "the work happened after the crash"
    );

    // Two boards over the record, as two processes would be, both told to accept.
    let mut also = Board::open(slice.home.path()).unwrap();
    slice
        .board
        .review("mason", &assigned.id, true, "checked")
        .unwrap();
    assert!(
        also.review("mason", &assigned.id, true, "checked").is_err(),
        "accepted twice"
    );

    assert_eq!(
        slice
            .records()
            .iter()
            .filter(|r| r.event.kind() == "reviewed")
            .count(),
        1
    );
    assert_eq!(
        slice.board.get(&assigned.id).unwrap().unwrap().attempts,
        1,
        "one submission, not one per restart"
    );
}

/// A conversation that cannot be resumed is a different loss from a process that died, and the
/// agent is the same agent through both.
#[test]
fn an_agent_that_loses_its_conversation_keeps_its_identity_and_its_work() {
    let mut slice = Slice::founded(&stand_in("agent-falls-over"));

    let assigned = task("mason", "nora", "write the file", "result.txt says done");
    slice.board.delegate("mason", &assigned).unwrap();

    let mut now = 1_000;
    let nora = slice.id("nora");
    let first_session = {
        slice.supervisor.tick(&slice.people, now).unwrap();
        slice.gone("nora");
        slice
            .supervisor
            .roll()
            .get(&nora)
            .unwrap()
            .session
            .clone()
            .expect("a conversation was pinned")
    };

    // Enough failures that retrying the resume is no longer the thing to vary.
    for _ in 0..carl::army::runtime::GIVE_UP_AFTER * 4 {
        let tick = slice.supervisor.tick(&slice.people, now).unwrap();
        slice.gone("nora");
        now = match tick.what.iter().find(|(n, _)| n == "nora").map(|(_, o)| o) {
            Some(carl::army::runtime::Outcome::Waiting { until }) => *until,
            _ => now + 1,
        };
        if slice
            .runtime_trail("nora")
            .contains(&"continuity_changed".to_string())
        {
            break;
        }
    }

    let record = slice.supervisor.roll().get(&nora).unwrap().clone();
    assert_eq!(record.agent, nora, "the same agent throughout");
    assert!(
        record.abandoned.contains(&first_session),
        "the conversation it gave up on is kept, because that transcript is the evidence"
    );
    assert_ne!(record.session, Some(first_session), "and it has a new one");

    let continuity = record.continuity.unwrap();
    assert_eq!(continuity.session, SessionContinuity::Replaced);
    assert!(continuity.degraded(), "and that is not reported as healthy");

    // None of which finished anything.
    let task = slice.board.get(&assigned.id).unwrap().unwrap();
    assert_eq!(task.status, Status::Assigned);
    assert_eq!(task.owner, "nora");
}

/// A worker whose task is blocked turns to the backup its lead approved, and not to anything
/// else, because there is nothing else it could reach.
#[test]
fn a_blocked_worker_falls_back_to_approved_work_and_only_that() {
    let slice = Slice::founded(&stand_in("agent-stays-up"));
    let mut board = Board::open(slice.home.path()).unwrap();

    let primary = task("mason", "nora", "the first job", "it is done");
    let backup = task("mason", "nora", "the approved backup", "it is done");
    board.delegate("mason", &primary).unwrap();
    board.delegate("mason", &backup).unwrap();
    board.advance("nora", &primary.id, Status::InHand).unwrap();

    assert!(
        board.backup_for("nora").unwrap().is_none(),
        "nothing wrong yet"
    );

    board.advance("nora", &primary.id, Status::Blocked).unwrap();
    let next = board.backup_for("nora").unwrap().expect("the approved one");
    assert_eq!(next.id, backup.id);

    // And she cannot approve herself another.
    let invented = Task::assign(
        "mason",
        "nora",
        "something I fancy doing",
        Verification::of(["done"]).unwrap(),
    )
    .unwrap();
    assert!(
        board.delegate("nora", &invented).is_err(),
        "a worker handed itself work"
    );
}

/// Nothing else may start agents in this home while one supervisor has it, which is what stops
/// two supervisors starting two processes for one agent.
#[test]
fn one_supervisor_per_home() {
    let slice = Slice::founded(&stand_in("agent-stays-up"));
    let second = Supervisor::take(slice.home.path(), stand_in("agent-stays-up"));
    assert!(second.is_err(), "a second supervisor took the same home");
}

/// The line the whole design is drawn on, checked from the supervisor's side.
#[test]
fn the_supervisor_can_carry_a_message_and_cannot_compose_one() {
    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));
    slice.supervisor.tick(&slice.people, 1_000).unwrap();

    let said = slice
        .supervisor
        .deliver(&slice.id("nora"), "hello", PATIENCE)
        .unwrap();
    assert!(said.contains("nothing to do about"), "{said}");

    // There is no task in the supervisor's vocabulary at all, so it cannot have an opinion
    // about one. This is the compiled version of that claim.
    let source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/army/runtime/supervisor.rs"),
    )
    .unwrap();
    for forbidden in ["Task", "Board", "delegate", "Status::"] {
        assert!(
            !source.contains(forbidden),
            "the supervisor mentions {forbidden}, so it has started to know about work"
        );
    }
}

/// An agent given up on is not an agent asleep, and waking one would restart the loop that
/// stopped in the first place.
#[test]
fn a_degraded_agent_cannot_be_woken() {
    let mut slice = Slice::founded(&stand_in("agent-falls-over"));
    let nora = slice.id("nora");

    let mut now = 1_000;
    for _ in 0..carl::army::runtime::GIVE_UP_AFTER * 4 {
        let tick = slice.supervisor.tick(&slice.people, now).unwrap();
        slice.gone("nora");
        now = match tick.what.iter().find(|(n, _)| n == "nora").map(|(_, o)| o) {
            Some(carl::army::runtime::Outcome::Waiting { until }) => *until,
            _ => now + 1,
        };
        if matches!(slice.lifecycle("nora"), Lifecycle::Degraded { .. }) {
            break;
        }
    }
    assert!(matches!(
        slice.lifecycle("nora"),
        Lifecycle::Degraded { .. }
    ));

    let e = slice
        .supervisor
        .wake(
            &nora,
            Because::Incident {
                what: "somebody wants her back".into(),
            },
            now,
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("given up on"), "{e}");
}

/// A wake always names what it is for, and there is no way to write one that does not.
#[test]
fn every_wake_says_what_it_is_for() {
    let mut slice = Slice::founded(&stand_in("agent-stays-up"));
    slice.supervisor.tick(&slice.people, 1_000).unwrap();
    let nora = slice.id("nora");
    slice.supervisor.stop(&nora, "overnight", 1_001).unwrap();

    let id = TaskId::quoted("some-task");
    slice
        .supervisor
        .wake(&nora, Because::Task { task: id.clone() }, 1_002)
        .unwrap();

    let woken = slice
        .records()
        .into_iter()
        .rfind(|r| r.event.kind() == "agent_woken")
        .expect("it was written down");
    let Event::AgentWoken { because, .. } = &woken.event else {
        panic!("wrong event");
    };
    assert_eq!(because.kind(), "task");
    assert_eq!(*because, Because::Task { task: id });
    assert_eq!(
        woken.actor, "supervisor",
        "carrying it out, not deciding it"
    );
}

/// Only whoever assigned a task decides what doing it is allowed to touch.
#[test]
fn a_worker_cannot_grant_itself_a_wider_workspace() {
    let slice = Slice::founded(&stand_in("agent-stays-up"));
    let mut board = Board::open(slice.home.path()).unwrap();

    let assigned = task("mason", "nora", "write the file", "result.txt says done");
    board.delegate("mason", &assigned).unwrap();

    let e = board
        .grant("nora", &assigned.id, "write anywhere I like")
        .unwrap_err()
        .to_string();
    assert!(e.contains("cannot grant anything"), "{e}");

    assert!(
        board
            .grant("mason", &assigned.id, "write under her folder")
            .is_ok(),
        "and her lead can"
    );
}

/// The panel used to say "unknown" about every process because nothing measured one. Something
/// does now, and the row has to say what the supervisor actually wrote rather than a default.
#[test]
fn the_panel_reports_what_the_supervisor_recorded_and_not_a_guess() {
    use carl::panel::{Maybe, ProcessState, snapshot};

    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));

    // Before any supervisor has run, nobody has said anything about any process.
    let before = snapshot::build(slice.home.path()).unwrap();
    assert!(
        before.agents.iter().all(|a| a.process.is_unknown()),
        "a process was claimed before anything was started"
    );
    assert!(before.agents.iter().all(|a| a.continuity.is_unknown()));

    slice.supervisor.tick(&slice.people, 1_000).unwrap();
    slice
        .supervisor
        .stop(&slice.id("mason"), "not needed tonight", 1_001)
        .unwrap();

    let after = snapshot::build(slice.home.path()).unwrap();
    let row = |name: &str| {
        after
            .agents
            .iter()
            .find(|a| a.name == name)
            .unwrap_or_else(|| panic!("no row for {name}"))
            .clone()
    };

    assert_eq!(row("nora").process, Maybe::known(ProcessState::Running));
    assert_eq!(
        row("mason").process,
        Maybe::known(ProcessState::Stopped),
        "asleep, which is not the same row as running and not the same as unknown"
    );

    let Maybe::Known { value: continuity } = row("nora").continuity else {
        panic!("her first start said nothing about what survived into it");
    };
    assert!(!continuity.degraded(), "a first process has lost nothing");
    assert!(continuity.describe().contains("first process"));

    // And the parent, role and current task are all there, which is what a row is for.
    assert_eq!(row("nora").reports_to.as_deref(), Some("mason"));
    assert_eq!(row("nora").rank, carl::army::Rank::Worker);
}

/// An agent that came back without its conversation must not read as an ordinary restart.
#[test]
fn the_panel_shows_a_lost_conversation_as_degraded() {
    use carl::panel::snapshot;

    let mut slice = Slice::founded(&stand_in("agent-falls-over"));

    let mut now = 1_000;
    for _ in 0..carl::army::runtime::GIVE_UP_AFTER * 4 {
        let tick = slice.supervisor.tick(&slice.people, now).unwrap();
        slice.gone("nora");
        now = match tick.what.iter().find(|(n, _)| n == "nora").map(|(_, o)| o) {
            Some(carl::army::runtime::Outcome::Waiting { until }) => *until,
            _ => now + 1,
        };
        if slice
            .runtime_trail("nora")
            .contains(&"continuity_changed".to_string())
        {
            break;
        }
    }

    let snapshot = snapshot::build(slice.home.path()).unwrap();
    let nora = snapshot.agents.iter().find(|a| a.name == "nora").unwrap();

    let carl::panel::Maybe::Known { value: continuity } = nora.continuity else {
        panic!("nothing was said about what she came back with");
    };
    assert_eq!(continuity.session, SessionContinuity::Replaced);
    assert!(continuity.degraded(), "and it is not shown as healthy");
}

/// Waking an agent that is already up used to clear its lifecycle, which made the next pass see
/// a record with no process behind it, start a second one resuming the same conversation, and
/// drop the first. That closed the pipe whoever was about to speak to it was holding, and it
/// showed up as the model failing to answer.
#[test]
fn waking_an_agent_that_is_already_up_does_nothing_at_all() {
    let mut slice = Slice::founded(&stand_in("agent-does-as-told"));
    slice.supervisor.tick(&slice.people, 1_000).unwrap();

    let nora = slice.id("nora");
    let before = slice.supervisor.roll().get(&nora).unwrap().clone();

    let woken = slice
        .supervisor
        .wake(
            &nora,
            Because::Task {
                task: TaskId::quoted("something"),
            },
            1_001,
        )
        .unwrap();
    assert!(!woken, "it said it woke an agent that was already awake");

    slice.supervisor.tick(&slice.people, 1_002).unwrap();
    let after = slice.supervisor.roll().get(&nora).unwrap().clone();
    assert_eq!(
        after.lifecycle.pid(),
        before.lifecycle.pid(),
        "a second process was started for an agent that already had one"
    );

    assert!(
        !slice
            .runtime_trail("nora")
            .contains(&"agent_woken".to_string()),
        "a wake nobody performed was written down as though it happened"
    );

    // And the pipe the caller was about to use still works.
    let said = slice.supervisor.deliver(&nora, "hello", PATIENCE).unwrap();
    assert!(said.contains("nothing to do about"), "{said}");
}
