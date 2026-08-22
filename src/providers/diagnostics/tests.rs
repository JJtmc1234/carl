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
fn a_missing_systemctl_costs_the_service_rows_and_nothing_else() {
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
    // Counted from the list rather than written down, because a unit added to the army is a
    // unit this test should already cover rather than a number somebody has to remember.
    assert_eq!(
        services.len(),
        crate::providers::army::services::CARL_UNITS.len()
    );
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

// ---- cost split, proved rigorously ----

/// The bug this design exists to prevent, at the scale it would have happened.
///
/// The old code ran three `systemctl` calls inside every `army()`. Six hundred frames is ten
/// seconds of a sixty frame panel, which would have been eighteen hundred child processes.
#[test]
fn six_hundred_frames_at_one_instant_start_one_set_of_processes() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());

    for _ in 0..600 {
        diagnostics.snapshot_at(1_000);
    }

    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "systemctl ran more than once"
    );
    assert_eq!(
        diagnostics.samples_taken(),
        1,
        "nvidia-smi ran more than once"
    );
}

/// The cheap half must keep working while the expensive half is held back, or the split has
/// simply made everything stale.
#[test]
fn cheap_reads_repeat_freely_while_expensive_probes_do_not() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());

    // Prime the caches so nothing is due.
    diagnostics.snapshot_at(1_000);
    assert_eq!(diagnostics.probes_taken(), 1);

    // A hundred more reads at the same instant. The records must follow the disk each time.
    for round in 0..100 {
        let records = diagnostics.records();
        assert!(
            records.iter().any(|x| x.component == "army.journal"),
            "round {round} lost the journal row"
        );
    }
    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "a cheap read started a process"
    );
    assert_eq!(diagnostics.samples_taken(), 1);
}

/// Records are genuinely reread rather than served from a cache, checked by changing the disk
/// underneath and taking no clock step at all.
#[test]
fn records_reread_the_journal_every_time() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());
    crate::army::personnel::found(d.path(), 1).unwrap();

    let before = diagnostics.army_at(1_000);
    let events = |rows: &[Diagnostic]| -> f64 {
        rows.iter()
            .find(|x| x.component == "army.journal")
            .and_then(|x| x.metrics.iter().find(|m| m.name == "events"))
            .and_then(|m| m.value.as_f64())
            .expect("the journal row counts events")
    };
    let first = events(&before);

    // Append to the journal without moving the clock.
    let mut journal = crate::army::event::Journal::open(d.path().join("run/events.jsonl")).unwrap();
    journal
        .append(
            "carl",
            crate::army::event::Event::Decided {
                task: None,
                what: "something happened".into(),
            },
        )
        .unwrap();
    drop(journal);

    let after = diagnostics.army_at(1_000);
    assert_eq!(
        events(&after),
        first + 1.0,
        "the record did not follow the file"
    );
    assert_eq!(diagnostics.probes_taken(), 1, "and no probe was rerun");
}

/// The two expensive sources have separate clocks, so a fast machine interval does not drag
/// the service probe along with it.
#[test]
fn the_machine_and_probe_intervals_are_independent() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path()).every(Intervals {
        machine_secs: 1,
        probe_secs: 10,
    });

    for second in 0..10 {
        diagnostics.snapshot_at(1_000 + second);
    }

    assert_eq!(
        diagnostics.samples_taken(),
        10,
        "the machine should have sampled once a second"
    );
    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "the slower probe should not have followed it"
    );

    diagnostics.snapshot_at(1_010);
    assert_eq!(diagnostics.probes_taken(), 2, "and it refreshes when due");
}

/// And the other way round, so neither is secretly driving the other.
#[test]
fn a_fast_probe_does_not_drag_the_machine_sampler() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path()).every(Intervals {
        machine_secs: 10,
        probe_secs: 1,
    });

    for second in 0..10 {
        diagnostics.snapshot_at(1_000 + second);
    }

    assert_eq!(diagnostics.probes_taken(), 10);
    assert_eq!(diagnostics.samples_taken(), 1);
}

#[test]
fn a_cache_refreshes_exactly_on_expiry_and_not_before() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path()).every(Intervals {
        machine_secs: 5,
        probe_secs: 5,
    });

    diagnostics.snapshot_at(1_000);
    assert_eq!(diagnostics.samples_taken(), 1);

    diagnostics.snapshot_at(1_004);
    assert_eq!(
        diagnostics.samples_taken(),
        1,
        "one second early is still cached"
    );

    diagnostics.snapshot_at(1_005);
    assert_eq!(
        diagnostics.samples_taken(),
        2,
        "exactly on expiry it refreshes"
    );
    assert_eq!(diagnostics.last_sampled_at(), Some(1_005));
}

/// A clock that jumps backwards, which happens when a machine syncs time, must not wedge the
/// cache shut forever.
#[test]
fn a_clock_that_goes_backwards_does_not_freeze_the_cache() {
    let d = tempfile::tempdir().unwrap();
    let mut diagnostics = at(d.path());

    diagnostics.machine_at(10_000);
    assert_eq!(diagnostics.samples_taken(), 1);

    // Saturating arithmetic means an earlier instant reads as no time passed, so it stays
    // cached rather than panicking or resampling on every frame.
    diagnostics.machine_at(9_000);
    assert_eq!(diagnostics.samples_taken(), 1);

    // And once the clock is ahead again by the interval, it recovers.
    diagnostics.machine_at(10_000 + diagnostics.intervals().machine_secs);
    assert_eq!(diagnostics.samples_taken(), 2);
}

/// A watched path that resolves to the root filesystem must not become a second root row.
#[test]
fn a_symlinked_home_does_not_duplicate_the_disk_it_points_at() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().canonicalize().unwrap();
    let link = real.join("link-to-self");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut through_link = Diagnostics::new(&link);
    let taken = through_link.snapshot_at(1_000);
    assert!(
        taken.duplicate_components().is_empty(),
        "duplicated: {:?}",
        taken.duplicate_components()
    );

    // The row is named by the resolved path, so two ways in are one component.
    let mut direct = Diagnostics::new(&real);
    let straight = direct.snapshot_at(1_000);
    let ids = |s: &Snapshot| -> Vec<String> {
        let mut v: Vec<String> = s
            .components()
            .iter()
            .filter(|c| c.starts_with("system.disk"))
            .map(|c| c.to_string())
            .collect();
        v.sort();
        v
    };
    assert_eq!(
        ids(&taken),
        ids(&straight),
        "the same filesystem, named twice"
    );
}
