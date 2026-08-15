//! What a snapshot costs when the caller is a render loop.
//!
//! The bug this exists to prevent was real and was measured: `army()` forked `systemctl` three
//! times per call, so a panel drawing at sixty frames a second would have started a hundred and
//! eighty processes a second. Process 3 fixed it and split the army half by cost. These tests
//! hold the seam from the backend's side, because the backend is the thing that will actually be
//! asked sixty times a second and the fix is only useful if the backend uses it properly.
//!
//! **The counters are the proof.** `samples_taken` and `probes_taken` are exposed precisely so
//! the limit can be checked from outside rather than believed, and asserting on them is stronger
//! than asserting on elapsed time, which would make these tests slow and flaky.

use std::path::Path;

use carl::army::personnel::found;
use carl::panel::Facts;
use carl::providers::diagnostics::{Diagnostics, Intervals};
use carl::providers::health::Kind;
use carl::providers::projects::Projects;

fn a_home(dir: &Path) -> Projects {
    found(dir, 1).unwrap();
    Projects::open(dir)
}

/// Sixty snapshots at one instant, which is one second of a panel at sixty frames.
#[test]
fn a_render_loop_worth_of_snapshots_probes_once_and_samples_once() {
    let dir = tempfile::tempdir().unwrap();
    let projects = a_home(dir.path());
    let mut diagnostics = Diagnostics::new(dir.path());

    let at = 1_755_200_000;
    for _ in 0..60 {
        let facts = Facts::gather_at(&mut diagnostics, &projects, &[], at);
        assert!(
            !facts.diagnostics.all().is_empty(),
            "still answers every time"
        );
    }

    assert_eq!(diagnostics.samples_taken(), 1, "nvidia-smi ran once");
    assert_eq!(diagnostics.probes_taken(), 1, "systemctl ran once");
}

/// And the limit lets go once the interval has actually passed.
///
/// A cache that never expires would pass the test above and be useless, so the other half has to
/// be checked too.
#[test]
fn the_limit_expires_rather_than_freezing_the_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let projects = a_home(dir.path());
    let intervals = Intervals {
        machine_secs: 2,
        probe_secs: 5,
    };
    let mut diagnostics = Diagnostics::new(dir.path()).every(intervals);

    let at = 1_755_200_000;
    Facts::gather_at(&mut diagnostics, &projects, &[], at);
    Facts::gather_at(&mut diagnostics, &projects, &[], at + 1);
    assert_eq!(diagnostics.samples_taken(), 1, "one second is inside two");

    Facts::gather_at(&mut diagnostics, &projects, &[], at + 2);
    assert_eq!(diagnostics.samples_taken(), 2, "and two seconds is not");
    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "the probe has its own, longer, limit"
    );

    Facts::gather_at(&mut diagnostics, &projects, &[], at + 5);
    assert_eq!(diagnostics.probes_taken(), 2);
}

/// The cheap half must not be held back by the expensive half's limit.
///
/// Army records are file reads. An event driven source that lags is not event driven, so a task
/// moving has to show up on the next snapshot even though nothing was resampled.
#[test]
fn the_cheap_army_half_is_never_stale() {
    let dir = tempfile::tempdir().unwrap();
    let diagnostics = Diagnostics::new(dir.path());
    found(dir.path(), 1).unwrap();

    let first = diagnostics.records();
    let again = diagnostics.records();
    assert_eq!(first.len(), again.len(), "read fresh every time");
    assert!(
        first.iter().all(|d| d.kind == Kind::EventDriven),
        "and still event driven in meaning"
    );
    assert!(
        first.iter().all(|d| d.measured_at.is_none()),
        "so none of them pretends to have been measured at an instant"
    );
}

/// Nothing may hand back a second row with the same name.
///
/// This found a real case: a home on the root filesystem produced two rows both called
/// `system.disk/`, which a panel keyed by component would have silently collapsed into one.
#[test]
fn every_component_id_is_unique() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Diagnostics::new(dir.path());
    found(dir.path(), 1).unwrap();

    let snapshot = diagnostics.snapshot_at(1_755_200_000);
    assert!(
        snapshot.duplicate_components().is_empty(),
        "duplicates: {:?}",
        snapshot.duplicate_components()
    );
}

/// Two families and no others, with `group` deriving the board rather than the panel guessing.
#[test]
fn every_component_belongs_to_army_or_system() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Diagnostics::new(dir.path());
    found(dir.path(), 1).unwrap();

    let snapshot = diagnostics.snapshot_at(1_755_200_000);
    for d in snapshot.all() {
        assert!(
            matches!(d.group(), "army" | "system"),
            "{} is in no family: group {}",
            d.component,
            d.group()
        );
        assert!(
            d.component.starts_with("army.") || d.component.starts_with("system."),
            "{} does not use a canonical prefix",
            d.component
        );
    }

    // And the old names are gone rather than merely unused.
    for stale in ["carl.", "agent.", "claude."] {
        assert!(
            !snapshot
                .all()
                .iter()
                .any(|d| d.component.starts_with(stale)),
            "{stale} is an old name and must not be produced"
        );
    }
}

/// A sampled row and an event driven row are different kinds of fact, and neither borrows the
/// other's freshness.
#[test]
fn the_two_kinds_keep_their_own_notion_of_time() {
    let dir = tempfile::tempdir().unwrap();
    let mut diagnostics = Diagnostics::new(dir.path());
    found(dir.path(), 1).unwrap();

    let snapshot = diagnostics.snapshot_at(1_755_200_000);
    for d in &snapshot.machine {
        assert_eq!(d.kind, Kind::Sampled);
        assert!(d.measured_at.is_some(), "{} lost its moment", d.component);
    }
    for d in &snapshot.army {
        assert_eq!(d.kind, Kind::EventDriven);
        assert!(
            d.measured_at.is_none(),
            "{} invented a moment it does not have",
            d.component
        );
    }
}

/// The backend must never flatten. That is a UI edge helper and it throws facts away.
#[test]
fn nothing_in_the_backend_flattens_a_diagnostic() {
    let source = std::fs::read_dir("src/panel")
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();

    // The word appears in a comment in view.rs explaining why the panel's own types were
    // deleted, so this looks for the call rather than the word.
    assert!(
        !source.contains(".flattened()"),
        "the backend's canonical state must keep every fact; flatten at the UI edge only"
    );
}
