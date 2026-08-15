//! The facade, driven the way a render loop will drive it.

use super::*;
use crate::providers::health::Kind;

fn at(home: &std::path::Path) -> Diagnostics {
    Diagnostics::new(home)
}

#[test]
fn a_snapshot_keeps_the_two_kinds_apart() {
    let d = tempfile::tempdir().unwrap();
    let taken = at(d.path()).snapshot_at(1_000);

    assert!(!taken.army.is_empty());
    assert!(!taken.machine.is_empty());

    for a in &taken.army {
        assert_eq!(a.kind, Kind::EventDriven, "{}", a.component);
        assert_eq!(a.measured_at, None, "{}", a.component);
    }
    for m in &taken.machine {
        assert_eq!(m.kind, Kind::Sampled, "{}", m.component);
        assert!(m.measured_at.is_some(), "{}", m.component);
    }
}

/// The thing this file exists for. Sixty calls in one frame must start one set of processes,
/// not sixty.
#[test]
fn a_render_loop_cannot_fork_a_process_every_frame() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());

    for _ in 0..60 {
        diagnostics.snapshot_at(1_000);
    }

    assert_eq!(
        diagnostics.samples_taken(),
        1,
        "nvidia-smi ran once, not sixty times"
    );
    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "systemctl ran once, not sixty times"
    );
}

/// And across a whole second of frames at the default interval, still once.
#[test]
fn frames_inside_the_interval_reuse_the_last_reading() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());
    let interval = diagnostics.intervals().machine_secs;

    let first = diagnostics.machine_at(1_000);
    for tick in 1..interval {
        diagnostics.machine_at(1_000 + tick);
    }
    assert_eq!(diagnostics.samples_taken(), 1);

    let again = diagnostics.machine_at(1_000 + interval - 1);
    assert_eq!(
        again[0].measured_at, first[0].measured_at,
        "the age is the original age, not a fresh looking one"
    );

    // Once the interval has passed it does resample, or the panel would freeze.
    diagnostics.machine_at(1_000 + interval);
    assert_eq!(diagnostics.samples_taken(), 2);
    assert_eq!(diagnostics.last_sampled_at(), Some(1_000 + interval));
}

#[test]
fn the_probe_has_its_own_slower_interval() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());
    let probe = diagnostics.intervals().probe_secs;
    assert!(probe >= diagnostics.intervals().machine_secs);

    diagnostics.probes_at(1_000);
    diagnostics.probes_at(1_000 + probe - 1);
    assert_eq!(diagnostics.probes_taken(), 1);

    diagnostics.probes_at(1_000 + probe);
    assert_eq!(diagnostics.probes_taken(), 2);
}

/// The cheap half must not be held back by the expensive one, or an event driven source stops
/// being event driven.
#[test]
fn records_are_never_cached_and_follow_a_change_immediately() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());

    let before = diagnostics.army_at(1_000);
    let personnel = before
        .iter()
        .find(|x| x.component == "army.personnel")
        .unwrap();
    assert_eq!(personnel.health, Health::Unknown, "no army yet");

    crate::army::personnel::found(d.path(), 1).unwrap();

    // Same instant, so nothing has been allowed to resample. The records still move.
    let after = diagnostics.army_at(1_000);
    let personnel = after
        .iter()
        .find(|x| x.component == "army.personnel")
        .unwrap();
    assert_eq!(
        personnel.health,
        Health::Healthy,
        "the folders are there now"
    );
    assert_eq!(diagnostics.probes_taken(), 1, "and no extra probe was run");
}

#[test]
fn an_interval_of_zero_is_refused_because_it_is_not_a_rate_limit() {
    let zero = Intervals {
        machine_secs: 0,
        probe_secs: 0,
    }
    .checked();
    assert_eq!(zero.machine_secs, 1);
    assert_eq!(zero.probe_secs, 1);

    let d = tempfile::tempdir().unwrap();
    let diagnostics = at(d.path()).every(Intervals {
        machine_secs: 0,
        probe_secs: 0,
    });
    assert_eq!(diagnostics.intervals().machine_secs, 1);
}

#[test]
fn a_caller_can_ask_for_a_faster_interval() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path()).every(Intervals {
        machine_secs: 1,
        probe_secs: 1,
    });

    diagnostics.machine_at(1_000);
    diagnostics.machine_at(1_001);
    assert_eq!(
        diagnostics.samples_taken(),
        2,
        "one second apart is two samples"
    );
}

/// The convention the panel groups on.
#[test]
fn every_component_id_is_army_or_system() {
    let d = tempfile::tempdir().unwrap();
    crate::army::personnel::found(d.path(), 1).unwrap();
    let taken = at(d.path()).snapshot_at(1_000);

    for id in taken.components() {
        assert!(
            id.starts_with("army.") || id.starts_with("system."),
            "{id} is neither an army nor a system component"
        );
    }

    // And the two boards are both populated.
    assert!(taken.components().iter().any(|id| id.starts_with("army.")));
    assert!(
        taken
            .components()
            .iter()
            .any(|id| id.starts_with("system."))
    );
}

/// A duplicate id would have the panel draw one row on top of another.
#[test]
fn no_two_components_share_an_id() {
    let d = tempfile::tempdir().unwrap();
    crate::army::personnel::found(d.path(), 1).unwrap();
    let taken = at(d.path()).snapshot_at(1_000);

    assert!(
        taken.duplicate_components().is_empty(),
        "duplicated: {:?}",
        taken.duplicate_components()
    );
}

/// The case that would have produced one: a home that is also the root filesystem.
#[test]
fn a_home_on_the_root_filesystem_does_not_duplicate_the_disk_component() {
    let mut diagnostics = Diagnostics::new("/");
    let taken = diagnostics.snapshot_at(1_000);
    assert!(
        taken.duplicate_components().is_empty(),
        "duplicated: {:?}",
        taken.duplicate_components()
    );
}

/// One source failing must cost one row, not the whole board.
#[test]
fn a_missing_systemctl_costs_three_rows_and_nothing_else() {
    let d = tempfile::tempdir().unwrap();
    let army = crate::providers::army::Army::new(d.path());

    let broken = army.processes_with("/nonexistent/systemctl", Some(1_000.0));
    let working = army.processes(Some(1_000.0));
    assert_eq!(
        broken.len(),
        working.len(),
        "the same rows are still produced"
    );

    let services: Vec<&Diagnostic> = broken
        .iter()
        .filter(|x| x.component.starts_with("army.service."))
        .collect();
    assert_eq!(services.len(), 3);
    for s in services {
        assert_eq!(s.health, Health::Unknown, "{}", s.component);
        assert!(s.summary.contains("did not answer"), "{}", s.summary);
    }

    // The process count is unaffected by systemctl being gone.
    assert!(
        broken
            .iter()
            .any(|x| x.component == "army.claude.processes"),
        "the other source still reported"
    );
}

#[test]
fn a_missing_graphics_card_is_unknown_rather_than_zero() {
    let card = crate::providers::system::gpu::read_gpu_with("/nonexistent/nvidia-smi");
    assert_eq!(card, None, "a binary that is not there answers nothing");
}

/// A home that cannot be read at all still produces a complete board.
#[test]
fn an_unreadable_home_still_produces_every_component() {
    let mut good = Diagnostics::new(tempfile::tempdir().unwrap().path().to_path_buf());
    let expected = good.snapshot_at(1_000).all().len();

    let mut bad = Diagnostics::new("/nonexistent/carl/home");
    let taken = bad.snapshot_at(1_000);
    assert_eq!(
        taken.all().len(),
        expected,
        "a missing home must not delete rows, only make them unknown"
    );
    assert!(
        taken.overall() != Health::Healthy,
        "and it is not pretending to be fine"
    );
}

#[test]
fn the_overall_health_is_the_worst_of_everything() {
    let d = tempfile::tempdir().unwrap();
    let taken = at(d.path()).snapshot_at(1_000);
    let worst = Health::worst(taken.all().into_iter().map(|x| x.health));
    assert_eq!(taken.overall(), worst);
}

#[test]
fn a_component_can_be_found_by_name() {
    let d = tempfile::tempdir().unwrap();
    let taken = at(d.path()).snapshot_at(1_000);

    assert!(taken.find("system.memory").is_some());
    assert!(taken.find("army.journal").is_some());
    assert!(taken.find("nothing.like.this").is_none());
}

#[test]
fn this_machine_reports_its_uptime() {
    let up = machine_uptime().expect("/proc/uptime is readable here");
    assert!(
        up > 0.0,
        "a machine up for no time at all is not running this"
    );
}
