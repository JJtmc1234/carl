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
    assert!(
        people.summary.contains(&format!(
            "{} agents",
            crate::army::org::everyone()
                .iter()
                .filter(|a| a.rank != crate::army::org::Rank::Human)
                .count()
        )),
        "{}",
        people.summary
    );

    for who in [
        "carl", "adrian", "iris", "evan", "mason", "nora", "olivia", "miles",
    ] {
        let agent = taken
            .iter()
            .find(|d| d.component == format!("army.agent.{who}"))
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
    let nora = taken
        .iter()
        .find(|d| d.component == "army.agent.nora")
        .unwrap();
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
    assert!(
        j.summary.contains(&format!(
            "{} events",
            crate::army::org::everyone()
                .iter()
                .filter(|a| a.rank != crate::army::org::Rank::Human)
                .count()
        )),
        "{}",
        j.summary
    );
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
            parent: None,
            must: vec!["it works".into()],
            project: None,
            workspace: None,
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
            taken
                .iter()
                .any(|d| d.component == format!("army.service.{unit}")),
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
        .find(|d| d.component == "army.claude.processes")
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

/// A visible `claude` process is a process and nothing more.
///
/// There is no authoritative mapping from a process to a named agent anywhere in this
/// codebase. `Pool::open_count` knows how many sessions one process is holding, and only
/// inside that process. So this reports a count and refuses to name anybody, because guessing
/// that some `claude` on the machine is Nora would be a label the panel could not defend.
#[test]
fn a_claude_process_is_never_labelled_with_an_agent_name() {
    let d = founded_home();
    let taken = Army::new(d.path()).snapshot(None);
    let claude = taken
        .iter()
        .find(|x| x.component == "army.claude.processes")
        .unwrap();

    for agent in crate::army::org::everyone() {
        assert!(
            !claude.summary.contains(agent.name),
            "the process count named {}: {}",
            agent.name,
            claude.summary
        );
        for m in &claude.metrics {
            assert!(
                !m.rendered().contains(agent.name),
                "metric {} named {}",
                m.name,
                agent.name
            );
        }
    }

    // What it does say is a count of processes, which is a fact.
    assert!(
        claude.summary.contains("claude process"),
        "{}",
        claude.summary
    );
}

/// The agent rows come from folders on disk, not from guessing which process is whose.
#[test]
fn an_agent_row_reports_only_what_its_folder_says() {
    let d = founded_home();
    let taken = Army::new(d.path()).snapshot(None);
    let nora = taken
        .iter()
        .find(|x| x.component == "army.agent.nora")
        .unwrap();

    // No pid, because nothing can prove which process is hers.
    assert!(
        !nora.metrics.iter().any(|m| m.name.contains("pid")),
        "an agent row claimed a process id"
    );
    assert_eq!(nora.summary, "idle", "which is what her folder says");
}

// ---- read only audit ----

/// The reported symptom, pinned against the shared API, now from the other side.
///
/// This test used to assert the opposite, and its own comment said what to do when that changed:
/// "if this ever stops being true, the guard in this module can go". It has stopped being true.
/// `Personnel::open` was the source of the empty `~/.carl/army` somebody saw appear after opening
/// a panel, and it no longer creates anything, so a home with no army reads as an army with no
/// folders. The guard below is kept anyway: it costs a directory check and it states the
/// provider's own promise rather than borrowing one from a module it does not own.
#[test]
fn the_shared_personnel_open_no_longer_creates_the_army_directory() {
    let d = tempfile::tempdir().unwrap();
    assert!(!d.path().join("army").exists());

    let people = personnel::Personnel::open(d.path()).unwrap();
    assert!(people.is_empty(), "there is nobody in it");
    assert!(
        !d.path().join("army").exists(),
        "reading a home with no army must leave it that way"
    );

    // And the other half, so the fix is not a read that quietly stopped working: founding still
    // creates what it is supposed to.
    personnel::found(d.path(), 1).unwrap();
    assert!(d.path().join("army/nora").exists());
}

/// And the provider never takes that path on a home with no army.
#[test]
fn no_army_diagnostic_creates_the_directory_it_is_looking_for() {
    let d = tempfile::tempdir().unwrap();
    let army = Army::new(d.path());

    // Every read only entry point this module has.
    army.founded();
    army.records();
    army.processes(Some(1_000.0));
    army.processes_with("/nonexistent/systemctl", None);
    army.snapshot(Some(1_000.0));
    army.snapshot(None);
    army.overall(None);

    assert!(
        !d.path().join("army").exists(),
        "a diagnostic founded an army by looking at one"
    );
    assert!(!d.path().join("run").exists(), "and wrote a journal");
    assert_eq!(
        std::fs::read_dir(d.path()).unwrap().count(),
        0,
        "the home should still be empty"
    );
}

/// The guard is a check on the directory, so it has to survive the directory existing but
/// being empty, which is exactly the state the reported symptom left behind.
#[test]
fn an_empty_army_directory_reads_as_an_army_with_nobody_in_it() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("army")).unwrap();

    let taken = Army::new(d.path()).snapshot(None);
    let people = taken
        .iter()
        .find(|x| x.component == "army.personnel")
        .unwrap();

    // Founded, so it loads. Nobody has a folder, so every agent is missing.
    assert_eq!(people.health, Health::Degraded, "{}", people.summary);
    assert!(people.summary.contains("0 agents"), "{}", people.summary);
    for who in [
        "carl", "adrian", "iris", "evan", "mason", "nora", "olivia", "miles",
    ] {
        assert!(people.summary.contains(who), "{} not named", who);
    }

    // And no agent rows are invented for folders that are not there.
    assert!(
        !taken.iter().any(|x| x.component.starts_with("army.agent.")),
        "an agent row appeared without a folder"
    );
}

/// Every unit this reports on has a file, and the installer installs it.
///
/// The failure this catches is silent, which is why it is worth a test that reads the
/// repository. A unit named here with no file cannot be installed, and a unit with a file that
/// the installer skips is never on the machine. Both look identical from the panel: a row
/// saying systemd did not answer, forever, which reads as a service that is down rather than as
/// one that was never installed.
#[test]
fn every_reported_unit_has_a_file_and_is_installed() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let installer = std::fs::read_to_string(repo.join("etc/systemd/install.sh")).unwrap();

    for unit in services::CARL_UNITS {
        let path = repo.join("etc/systemd").join(format!("{unit}.service"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} is missing: {e}", path.display()));

        // Without this `systemctl enable` fails, and the unit is only ever running until the
        // next reboot, which is the sort of thing nobody notices until the reboot.
        assert!(
            text.contains("[Install]"),
            "{unit} has no [Install] section, so it cannot be enabled"
        );
        assert!(
            installer.contains(unit),
            "install.sh never mentions {unit}, so it is never put on the machine"
        );
    }
}
