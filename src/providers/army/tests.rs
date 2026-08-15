//! The army diagnostics, against real files.

use super::*;
use crate::army::event::{Event, Journal};
use crate::army::personnel;
use crate::army::task::TaskId;

fn empty_home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn founded_home() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    personnel::found(d.path(), 1_000).expect("found the army");
    d
}

/// The state this machine is actually in today, and it must not read as a failure.
#[test]
fn a_home_with_no_army_says_so_rather_than_failing() {
    let d = empty_home();
    let army = Army::new(d.path());
    assert!(!army.founded());

    let taken = army.snapshot(None);
    let people = taken
        .iter()
        .find(|d| d.component == "army.personnel")
        .unwrap();
    assert_eq!(people.health, Health::Unknown);
    assert!(people.summary.contains("no army"), "{}", people.summary);
    assert_eq!(
        people.metrics[0].rendered(),
        "unknown",
        "never a count of zero"
    );
}

/// Looking at something must not change it. `Personnel::open` creates the directory, so the
/// provider has to check first or a panel would found an army by being opened.
#[test]
fn reading_an_unfounded_army_does_not_create_it() {
    let d = empty_home();
    let army = Army::new(d.path());
    army.snapshot(None);
    army.overall(None);

    assert!(
        !d.path().join("army").exists(),
        "the panel founded an army by looking"
    );
    assert!(!d.path().join("run").exists());
}

#[test]
fn a_founded_army_reports_its_agents() {
    let d = founded_home();
    let army = Army::new(d.path());
    let taken = army.snapshot(None);

    let people = taken
        .iter()
        .find(|d| d.component == "army.personnel")
        .unwrap();
    assert_eq!(people.health, Health::Healthy);
    assert!(people.summary.contains("4 agents"), "{}", people.summary);

    for who in ["carl", "adrian", "mason", "nora"] {
        let agent = taken
            .iter()
            .find(|d| d.component == format!("agent.{who}"))
            .unwrap_or_else(|| panic!("{who} should be reported"));
        assert_eq!(agent.summary, "idle", "nobody has been given anything yet");
        assert_eq!(agent.kind, Kind::EventDriven);
    }
}

#[test]
fn an_agent_holding_a_task_says_which_one() {
    let d = founded_home();
    let mut people = personnel::Personnel::open(d.path()).unwrap();
    people
        .update_state("nora", |s| s.take_up(&TaskId::quoted("task-7"), 2_000))
        .unwrap();
    drop(people);

    let taken = Army::new(d.path()).snapshot(None);
    let nora = taken.iter().find(|d| d.component == "agent.nora").unwrap();
    assert!(nora.summary.contains("task-7"), "{}", nora.summary);
}

/// A missing folder is one agent without state, not a broken organisation.
#[test]
fn an_agent_without_a_folder_is_degraded_rather_than_failed() {
    let d = founded_home();
    std::fs::remove_dir_all(d.path().join("army").join("mason")).unwrap();

    let taken = Army::new(d.path()).snapshot(None);
    let people = taken
        .iter()
        .find(|d| d.component == "army.personnel")
        .unwrap();
    assert_eq!(people.health, Health::Degraded);
    assert!(people.summary.contains("mason"), "{}", people.summary);
}

#[test]
fn folders_that_will_not_load_are_a_failure() {
    let d = founded_home();
    std::fs::write(
        d.path().join("army").join("nora").join("config.json"),
        "{ no",
    )
    .unwrap();

    let taken = Army::new(d.path()).snapshot(None);
    let people = taken
        .iter()
        .find(|d| d.component == "army.personnel")
        .unwrap();
    assert_eq!(people.health, Health::Failed);
}

#[test]
fn a_journal_that_was_never_written_is_unknown_rather_than_empty() {
    let d = empty_home();
    let taken = Army::new(d.path()).snapshot(None);
    let j = taken
        .iter()
        .find(|d| d.component == "army.journal")
        .unwrap();
    assert_eq!(j.health, Health::Unknown);
    assert_eq!(j.metrics[0].rendered(), "unknown");
}

#[test]
fn the_founding_journal_is_read_back() {
    let d = founded_home();
    let taken = Army::new(d.path()).snapshot(None);
    let j = taken
        .iter()
        .find(|d| d.component == "army.journal")
        .unwrap();

    assert_eq!(j.health, Health::Healthy);
    assert!(j.summary.contains("4 events"), "{}", j.summary);
}

/// Nothing in hand is not the same as nothing measurable, so latency is unknown while tasks
/// are healthy.
#[test]
fn an_army_with_no_history_has_no_latency_to_report() {
    let d = founded_home();
    let taken = Army::new(d.path()).snapshot(None);

    let tasks = taken.iter().find(|d| d.component == "army.tasks").unwrap();
    assert_eq!(tasks.health, Health::Healthy);
    assert_eq!(tasks.summary, "nothing in hand");

    let latency = taken
        .iter()
        .find(|d| d.component == "army.latency")
        .unwrap();
    assert_eq!(latency.health, Health::Unknown);
    for m in &latency.metrics {
        if m.name.contains("handover") {
            assert_eq!(
                m.rendered(),
                "unknown",
                "no handover is not a handover of zero seconds"
            );
        }
    }
}

/// The whole point of the four state model, end to end from a real journal file.
#[test]
fn a_blocked_task_turns_the_task_diagnostic_blocked() {
    let d = founded_home();
    let task = TaskId::quoted("stuck");
    let mut j = Journal::open(Army::new(d.path()).journal_path()).unwrap();

    j.append(
        "mason",
        Event::Delegated {
            task: task.clone(),
            to: "nora".into(),
            goal: "the hard one".into(),
        },
    )
    .unwrap();
    for attempt in 1..=crate::army::task::MAX_ATTEMPTS {
        j.append(
            "nora",
            Event::Submitted {
                task: task.clone(),
                attempt,
                words: 10,
            },
        )
        .unwrap();
        j.append(
            "mason",
            Event::Reviewed {
                task: task.clone(),
                accepted: false,
                why: "not verified".into(),
            },
        )
        .unwrap();
    }
    j.append(
        "mason",
        Event::moved(
            &task,
            crate::army::Status::Submitted,
            crate::army::Status::ChangesRequested,
        ),
    )
    .unwrap();
    drop(j);

    let taken = Army::new(d.path()).snapshot(None);
    let tasks = taken.iter().find(|d| d.component == "army.tasks").unwrap();
    assert_eq!(tasks.health, Health::Blocked, "{}", tasks.summary);
    assert!(tasks.summary.contains("1 blocked"), "{}", tasks.summary);

    // And the army as a whole is now blocked rather than healthy.
    assert_eq!(Army::new(d.path()).overall(None), Health::Blocked);
}

/// Every army diagnostic is derived from something that happened, so none of it is telemetry.
#[test]
fn nothing_from_the_army_is_labelled_sampled() {
    let d = founded_home();
    for diagnostic in Army::new(d.path()).snapshot(Some(1000.0)) {
        assert_eq!(
            diagnostic.kind,
            Kind::EventDriven,
            "{} is derived from the record, not read off a clock",
            diagnostic.component
        );
        assert_eq!(diagnostic.measured_at, None, "{}", diagnostic.component);
    }
}

#[test]
fn the_carl_services_are_included_in_the_snapshot() {
    let d = empty_home();
    let taken = Army::new(d.path()).snapshot(None);
    for unit in services::CARL_UNITS {
        assert!(
            taken.iter().any(|d| d.component == format!("carl.{unit}")),
            "{unit} should be reported"
        );
    }
}

#[test]
fn claude_processes_are_counted_from_proc() {
    let d = empty_home();
    let taken = Army::new(d.path()).snapshot(None);
    let claude = taken
        .iter()
        .find(|d| d.component == "claude.processes")
        .unwrap();
    assert!(matches!(claude.health, Health::Healthy | Health::Degraded));
    // Nothing is asserted about the count, because it depends on what is running. What is
    // asserted is that with no processes the memory figure is a gap rather than zero.
    let resident = claude
        .metrics
        .iter()
        .find(|m| m.name == "resident")
        .unwrap();
    let count = claude
        .metrics
        .iter()
        .find(|m| m.name == "processes")
        .unwrap();
    if count.value.as_f64() == Some(0.0) {
        assert_eq!(resident.rendered(), "unknown");
    }
}

#[test]
fn a_long_silence_in_the_journal_is_noticeable() {
    let folded = journal::Folded {
        last_at: Some(1_000),
        ..journal::Folded::default()
    };
    assert!(journal_is_quiet(&folded, 1_000 + QUIET_JOURNAL_SECS + 1));
    assert!(!journal_is_quiet(&folded, 1_000 + 60));
    assert!(
        !journal_is_quiet(&journal::Folded::default(), 9_999_999),
        "a journal with no entries has not gone quiet, it has never spoken"
    );
}
