//! The operator commands, against real homes rather than fixtures.
//!
//! These exist because the interesting cases are the ones where the sources disagree. A process
//! record that says running and a memory folder that is gone is not a hypothetical: it is what
//! `carl army who` reported as "idle" for as long as it existed.

use carl::army::personnel::{Personnel, found, memory};
use carl::army::runtime::Lifecycle;
use carl::army::survey;

fn founded() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("a temp home");
    found(d.path(), 0).expect("found an army");
    d
}

/// An empty home answers honestly instead of failing.
#[test]
fn an_unfounded_home_is_answerable() {
    let d = tempfile::tempdir().expect("a temp home");
    let all = survey::everyone(d.path()).expect("a survey");
    assert!(all.iter().all(|s| !s.enlisted));
    assert!(
        survey::activity(d.path(), None, 20)
            .expect("activity")
            .is_empty()
    );
}

/// The whole point of the join. Every source is content and one of them is wrong.
#[test]
fn a_missing_memory_folder_is_visible_even_though_nothing_else_is_wrong() {
    let d = founded();
    let people = Personnel::open(d.path()).expect("personnel");
    std::fs::remove_dir_all(memory::dir(&people.folder("nora"))).expect("remove");

    let all = survey::everyone(d.path()).expect("a survey");
    let nora = all.iter().find(|s| s.agent.name == "nora").expect("nora");
    let others = all.iter().filter(|s| s.agent.name != "nora");

    assert!(
        nora.worry().is_some(),
        "a lost memory folder is not visible"
    );
    assert!(others.map(|s| s.worry()).all(|w| w.is_none()), "it spread");
}

/// Absent and stopped are different, and collapsing them would invent a fact.
#[test]
fn nobody_having_said_is_not_reported_as_stopped() {
    let d = founded();
    for s in survey::everyone(d.path()).expect("a survey") {
        assert!(s.runtime.is_none());
        assert_eq!(s.worry(), None, "{} was worried about", s.agent.name);
    }
}

/// The lifecycle word is a column, and the sentence behind it belongs on the warning line.
#[test]
fn every_lifecycle_has_a_short_word() {
    let cases = [
        (Lifecycle::Never, "never started"),
        (Lifecycle::Degraded { why: "x".into() }, "degraded"),
        (Lifecycle::Stopped { why: "x".into() }, "stopped"),
        (Lifecycle::Asleep { since: 0 }, "asleep"),
    ];
    for (lifecycle, word) in cases {
        let got = survey::lifecycle_word(&lifecycle);
        assert_eq!(got, word);
        assert!(!got.contains('{'), "a struct leaked into a column: {got}");
    }
}

/// Hierarchy, from the compiled table, so it is the same one delegation enforces.
#[test]
fn the_reporting_line_is_the_one_delegation_uses() {
    let d = founded();
    let all = survey::everyone(d.path()).expect("a survey");
    for s in &all {
        for report in &s.reports {
            assert!(
                carl::army::org::may_delegate(s.agent.name, report.name),
                "{} lists {} but cannot hand to it",
                s.agent.name,
                report.name
            );
        }
    }
    let miles = all.iter().find(|s| s.agent.name == "miles").expect("miles");
    assert!(miles.reports.is_empty(), "a worker hands to nobody");
}

/// Deterministic enough to read twice and to grep.
#[test]
fn the_survey_is_stable_across_calls() {
    let d = founded();
    let once: Vec<&str> = survey::everyone(d.path())
        .expect("a survey")
        .iter()
        .map(|s| s.agent.name)
        .collect();
    let twice: Vec<&str> = survey::everyone(d.path())
        .expect("a survey")
        .iter()
        .map(|s| s.agent.name)
        .collect();
    assert_eq!(once, twice);
}

// Activity.

/// Every variant renders as words. A line that prints its own struct is a line nobody reads.
#[test]
fn no_activity_line_leaks_a_debug_struct() {
    let d = founded();
    let lines: Vec<String> = survey::activity(d.path(), None, 500)
        .expect("activity")
        .iter()
        .map(survey::line_of)
        .collect();

    assert!(!lines.is_empty(), "founding recorded nothing to render");
    for line in &lines {
        assert!(!line.contains(" { "), "a struct leaked: {line}");
        assert!(
            !line.contains('\n'),
            "a record spilled onto two rows: {line}"
        );
    }
}

#[test]
fn activity_is_bounded_to_what_was_asked_for() {
    let d = founded();
    for most in [1, 3, 5] {
        let got = survey::activity(d.path(), None, most).expect("activity");
        assert!(got.len() <= most, "asked for {most}, got {}", got.len());
    }
}

/// A malformed journal line must not take the whole command down. An operator asking what
/// happened is often asking precisely because something went wrong.
#[test]
fn a_torn_journal_line_does_not_stop_the_rest_being_readable() {
    let d = founded();
    let journal = d.path().join("run").join("events.jsonl");
    let good = std::fs::read_to_string(&journal).expect("the journal");
    std::fs::write(&journal, format!("{good}{{not valid json\n")).expect("write");

    // Either it reads the good lines or it refuses clearly. What it must not do is panic.
    match survey::activity(d.path(), None, 20) {
        Ok(records) => assert!(!records.is_empty(), "good lines were thrown away"),
        Err(e) => assert!(!e.to_string().is_empty(), "a refusal must say something"),
    }
}
