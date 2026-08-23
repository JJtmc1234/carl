use super::*;
use crate::army::board::Board;
use crate::army::event::{Because, Journal};
use crate::army::runtime::{Continuity, Memory, Process, Session};
use crate::army::task::{Task, Verification};

fn verification() -> Verification {
    Verification::of(["it works"]).unwrap()
}

/// An agent id in the shape the validator insists on, from one distinguishing digit.
fn agent(mark: &str) -> AgentId {
    AgentId::new(format!("a-{}", mark.repeat(16))).unwrap()
}

/// A journal in a temporary home, written through the real appender.
///
/// Built rather than hand written, so a change to the event vocabulary breaks these tests in
/// the same place it breaks everything else, instead of leaving a fixture that quietly still
/// parses and no longer means what it says.
struct Log {
    _dir: tempfile::TempDir,
    journal: Journal,
    board: Board,
}

impl Log {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.jsonl");
        Self {
            journal: Journal::open(&path).unwrap(),
            board: Board::at(&path).unwrap(),
            _dir: dir,
        }
    }

    /// A supervisor event, which no board writes.
    fn put(&mut self, actor: &str, event: Event) -> &mut Self {
        self.journal.append(actor, event).unwrap();
        self
    }

    /// Work, through the thing that actually records it.
    ///
    /// The board rather than a hand written `Delegated` line, because status comes off the
    /// `Moved` events the board writes alongside it. A fixture that wrote the lines by hand
    /// would be testing the fold against a shape nothing produces, which is how a measure ends
    /// up correct against the test and wrong against the record.
    fn delegate(&mut self, by: &str, task: &Task) -> &mut Self {
        self.board.delegate(by, task).unwrap();
        self
    }

    fn carry(&mut self, by: &str, task: &Task) -> &mut Self {
        self.board.advance(by, &task.id, Status::InHand).unwrap();
        self
    }

    fn submit(&mut self, by: &str, task: &Task) -> &mut Self {
        self.board.submit(by, &task.id, 20).unwrap();
        self
    }

    fn review(&mut self, by: &str, task: &Task, accepted: bool) -> &mut Self {
        self.board
            .review(by, &task.id, accepted, "checked")
            .unwrap();
        self
    }

    fn read(&self) -> Vec<Record> {
        crate::army::event::read(self.journal.path()).unwrap()
    }

    fn metrics(&self) -> Metrics {
        of(&self.read())
    }
}

/// The one measure that says the thing worked.
#[test]
fn an_objective_carried_all_the_way_through_counts_as_accepted() {
    let mut log = Log::new();
    let objective = Task::assign("jj", "carl", "make it faster", verification()).unwrap();
    let concrete = Task::split_from(
        &objective,
        "carl",
        "mason",
        "the factorio half",
        verification(),
    )
    .unwrap();

    log.delegate("jj", &objective)
        .carry("carl", &objective)
        .delegate("carl", &concrete)
        .carry("mason", &concrete)
        .submit("mason", &concrete)
        .review("carl", &concrete, true)
        .submit("carl", &objective)
        .review("jj", &objective, true);

    let m = log.metrics();
    assert_eq!(m.objectives.len(), 1, "the subtask is not an objective");
    assert_eq!(m.objectives[0].goal, "make it faster");
    assert!(m.objectives[0].accepted());
    assert!(m.objectives[0].unattended(), "JJ never had to come back");
    assert_eq!(m.accepted(), 1);
    assert_eq!(m.reviews.accepted, 2);
    assert_eq!(m.reviews.rejected, 0);
}

/// Asking for something is not intervening in it.
///
/// Counting the opening objective would put a floor of one under the number whose entire
/// purpose is to reach zero, so an army that behaved perfectly could never say so.
#[test]
fn opening_an_objective_is_not_an_intervention() {
    let mut log = Log::new();
    let objective = Task::assign("jj", "carl", "make it faster", verification()).unwrap();

    log.put(
        "jj",
        Event::Intervened {
            what: Intervention::Objective {
                what: "make it faster".into(),
            },
        },
    )
    .delegate("jj", &objective);

    let m = log.metrics();
    assert_eq!(m.objectives[0].interventions, 0);
    assert_eq!(m.loose_interventions, 0);
    assert_eq!(m.interventions_each(), Some(0.0));
}

/// An intervention deep in the tree belongs to the thing JJ actually asked for.
#[test]
fn an_intervention_on_a_subtask_counts_against_its_objective() {
    let mut log = Log::new();
    let objective = Task::assign("jj", "carl", "make it faster", verification()).unwrap();
    let department = Task::split_from(
        &objective,
        "carl",
        "mason",
        "the factorio half",
        verification(),
    )
    .unwrap();
    let concrete = Task::split_from(
        &department,
        "mason",
        "nora",
        "cache the lookup",
        verification(),
    )
    .unwrap();

    log.delegate("jj", &objective)
        .delegate("carl", &department)
        .delegate("mason", &concrete)
        .put(
            "jj",
            Event::Intervened {
                what: Intervention::Stopped {
                    task: concrete.id.clone(),
                    why: "wrong approach".into(),
                },
            },
        );

    let m = log.metrics();
    assert_eq!(
        m.objectives[0].interventions, 1,
        "counted three levels up, against the objective"
    );
    assert!(!m.objectives[0].unattended());
    assert_eq!(m.loose_interventions, 0);
}

/// An intervention that names no task is kept rather than spread over whatever was open.
#[test]
fn an_intervention_naming_no_task_is_counted_on_its_own() {
    let mut log = Log::new();
    let objective = Task::assign("jj", "carl", "make it faster", verification()).unwrap();

    log.delegate("jj", &objective)
        .put(
            "jj",
            Event::Intervened {
                what: Intervention::Message {
                    to: "nora".into(),
                    what: "stop what you are doing".into(),
                },
            },
        )
        .put(
            "jj",
            Event::Intervened {
                what: Intervention::Override {
                    agent: "nora".into(),
                    instruction: "never touch that file".into(),
                },
            },
        );

    let m = log.metrics();
    assert_eq!(m.loose_interventions, 2);
    assert_eq!(
        m.objectives[0].interventions, 0,
        "guessing which objective a sentence belonged to would invent a figure"
    );
}

/// A rejection and the second attempt behind it, which is what the two rates are made of.
#[test]
fn a_rejection_and_the_retry_that_follows_are_both_counted() {
    let mut log = Log::new();
    let task = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();

    log.delegate("mason", &task)
        .carry("nora", &task)
        .submit("nora", &task)
        .review("mason", &task, false)
        .carry("nora", &task)
        .submit("nora", &task)
        .review("mason", &task, true);

    let m = log.metrics();
    assert_eq!(m.reviews.rejected, 1);
    assert_eq!(m.reviews.accepted, 1);
    assert_eq!(m.retries.submissions, 2);
    assert_eq!(m.retries.repeats, 1, "only the second attempt is a repeat");
}

/// A crash answered by a start is recovery. A crash answered by nothing is not.
#[test]
fn a_crash_is_only_recovered_when_the_same_agent_starts_again() {
    let mut log = Log::new();
    let nora = agent("1");
    let evan = agent("2");

    log.put(
        "supervisor",
        Event::AgentCrashed {
            agent: nora.clone(),
            name: "nora".into(),
            code: Some(1),
            attempt: 1,
        },
    )
    .put(
        "supervisor",
        Event::AgentStarted {
            agent: nora.clone(),
            name: "nora".into(),
            continuity: Continuity {
                process: Process::Replaced,
                session: Session::Resumed,
                memory: Memory::Kept,
            },
            attempt: 1,
        },
    )
    .put(
        "supervisor",
        Event::AgentCrashed {
            agent: evan.clone(),
            name: "evan".into(),
            code: Some(1),
            attempt: 1,
        },
    );

    let m = log.metrics();
    assert_eq!(m.recovery.crashes, 2);
    assert_eq!(m.recovery.resumed, 1, "nora came back and evan has not");
    assert_eq!(m.recovery.outstanding, 1);
    assert_eq!(m.recovery.gave_up, 0);
}

/// Giving up answers a crash without recovering from it, and the two must not both count.
#[test]
fn giving_up_answers_a_crash_without_recovering_it() {
    let mut log = Log::new();
    let nora = agent("1");

    log.put(
        "supervisor",
        Event::AgentCrashed {
            agent: nora.clone(),
            name: "nora".into(),
            code: Some(1),
            attempt: 4,
        },
    )
    .put(
        "supervisor",
        Event::AgentGaveUp {
            agent: nora.clone(),
            name: "nora".into(),
            why: "four starts, four exits".into(),
        },
    );

    let m = log.metrics();
    assert_eq!(m.recovery.crashes, 1);
    assert_eq!(m.recovery.gave_up, 1);
    assert_eq!(m.recovery.resumed, 0);
    assert_eq!(
        m.recovery.outstanding, 0,
        "somebody has been told, which is what outstanding means"
    );
}

/// The refusals and the losses, which are the half of the record worth reading.
#[test]
fn refusals_and_continuity_losses_are_counted() {
    let mut log = Log::new();
    let nora = agent("1");

    log.put(
        "carl",
        Event::Refused {
            what: "assign work to nora".into(),
            why: "carl cannot hand work straight to nora".into(),
        },
    )
    .put(
        "supervisor",
        Event::ContinuityChanged {
            agent: nora.clone(),
            name: "nora".into(),
            from: Session::Resumed,
            to: Session::Replaced,
            why: "the transcript would not resume".into(),
            abandoned: None,
        },
    )
    .put(
        "adrian",
        Event::EmergencyDeclared {
            task: crate::army::task::TaskId::quoted("t-1"),
            why: "nobody else is up".into(),
        },
    );

    let m = log.metrics();
    assert_eq!(m.refusals, 1);
    assert_eq!(m.continuity_failures, 1);
    assert_eq!(m.escalations, 1);
}

/// An average over nothing is not zero.
///
/// Reporting it as zero would give an army that has never been asked to do anything the best
/// possible score on the measure that matters most.
#[test]
fn an_empty_record_reports_no_average_rather_than_a_perfect_one() {
    let m = of(&[]);
    assert!(m.objectives.is_empty());
    assert_eq!(m.interventions_each(), None);
    assert_eq!(m.accepted(), 0);
    assert_eq!(m.unattended(), 0);
}

/// The trend is in the recent ones, so the window has to work before there are enough.
#[test]
fn the_recent_window_is_the_whole_record_until_there_is_more_of_it() {
    let mut log = Log::new();
    for n in 0..3 {
        let objective =
            Task::assign("jj", "carl", format!("objective {n}"), verification()).unwrap();
        log.delegate("jj", &objective);
    }

    let m = log.metrics();
    assert_eq!(m.latest(10).len(), 3, "not padded, and not empty");
    assert_eq!(m.latest(2).len(), 2);
    assert_eq!(m.latest(2)[0].goal, "objective 1", "the most recent two");
}

/// Objectives come back in the order they were opened, because a trend needs an order.
#[test]
fn objectives_are_in_the_order_they_were_opened() {
    let mut log = Log::new();
    let first = Task::assign("jj", "carl", "first", verification()).unwrap();
    let second = Task::assign("jj", "carl", "second", verification()).unwrap();

    log.delegate("jj", &first).delegate("jj", &second);

    let m = log.metrics();
    let goals: Vec<&str> = m.objectives.iter().map(|o| o.goal.as_str()).collect();
    assert_eq!(goals, vec!["first", "second"]);
}

/// The supervisor's own events say nothing about work, and must not land in an objective.
#[test]
fn waking_an_agent_is_not_an_intervention() {
    let mut log = Log::new();
    let objective = Task::assign("jj", "carl", "make it faster", verification()).unwrap();

    log.delegate("jj", &objective).put(
        "supervisor",
        Event::AgentWoken {
            agent: agent("1"),
            name: "nora".into(),
            because: Because::Task {
                task: objective.id.clone(),
            },
        },
    );

    let m = log.metrics();
    assert_eq!(m.objectives[0].interventions, 0);
    assert_eq!(m.loose_interventions, 0);
}
