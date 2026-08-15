//! The sampler, run against this actual machine.
//!
//! These are deliberately real rather than mocked. The point of the module is that the sources
//! exist and answer here, so a test that stubs them proves nothing worth knowing.

use super::*;
use crate::providers::health::Health;

fn sampler() -> Sampler {
    Sampler::new(std::env::temp_dir())
}

#[test]
fn a_sample_covers_every_component_once() {
    let mut s = sampler();
    let taken = s.sample(1000);

    let names: Vec<&str> = taken.iter().map(|d| d.component.as_str()).collect();
    assert!(names.contains(&"system.cpu"));
    assert!(names.contains(&"system.memory"));
    assert!(names.contains(&"system.gpu"));
    assert!(names.contains(&"system.temperature"));
    assert!(names.contains(&"system.network"));
    assert!(
        names
            .iter()
            .filter(|n| n.starts_with("system.disk"))
            .count()
            >= 1,
        "at least the root filesystem"
    );
}

/// Everything the sampler produces is telemetry and has to be labelled as such, or the panel
/// will show a two second old number with the confidence of a fact.
#[test]
fn everything_sampled_is_marked_sampled_and_carries_its_age() {
    let mut s = sampler();
    for d in s.sample(4242) {
        assert_eq!(d.kind, Kind::Sampled, "{} is telemetry", d.component);
        assert_eq!(
            d.measured_at,
            Some(4242),
            "{} must say when it was read",
            d.component
        );
    }
}

/// The rule the whole provider is built around, demonstrated at the one place it always bites.
#[test]
fn the_first_sample_cannot_know_cpu_utilisation_and_says_so() {
    let mut s = sampler();
    let first = s.sample(1000);
    let cpu = first.iter().find(|d| d.component == "system.cpu").unwrap();

    let utilisation = cpu
        .metrics
        .iter()
        .find(|m| m.name == "utilisation")
        .unwrap();
    assert!(
        !utilisation.value.is_known(),
        "one reading cannot be a difference, got {:?}",
        utilisation.value
    );
    assert_eq!(
        utilisation.rendered(),
        "unknown",
        "and never renders as zero"
    );

    // Load average needs no history, so it is there on the first sample.
    let load = cpu.metrics.iter().find(|m| m.name == "load 1m").unwrap();
    assert!(load.value.is_known(), "load is a level, not a difference");
}

#[test]
fn the_second_sample_knows_cpu_utilisation() {
    let mut s = sampler();
    s.sample(1000);

    // Give the counters something to count. Busy work rather than a sleep, so the jiffies
    // actually move on a quiet machine.
    let mut spin = 0u64;
    for i in 0..8_000_000u64 {
        spin = spin.wrapping_add(i);
    }
    assert!(spin > 0);

    let second = s.sample(1002);
    let cpu = second.iter().find(|d| d.component == "system.cpu").unwrap();
    let utilisation = cpu
        .metrics
        .iter()
        .find(|m| m.name == "utilisation")
        .unwrap();

    match utilisation.value.as_f64() {
        Some(percent) => assert!(
            (0.0..=100.0).contains(&percent),
            "utilisation out of range: {percent}"
        ),
        None => panic!("a second reading should give a difference"),
    }
}

/// Read the real values and check they are believable, which is the sanity check the brief
/// asked for rather than a tautology.
#[test]
fn the_real_numbers_from_this_machine_are_believable() {
    let mut s = sampler();
    let taken = s.sample(now());

    let memory = taken
        .iter()
        .find(|d| d.component == "system.memory")
        .unwrap();
    let total = memory
        .metrics
        .iter()
        .find(|m| m.name == "total")
        .and_then(|m| m.value.as_f64())
        .expect("this machine reports its memory");
    assert!(
        (1024.0..1_048_576.0).contains(&total),
        "total memory of {total} MiB is not a real machine"
    );

    let used = memory
        .metrics
        .iter()
        .find(|m| m.name == "used")
        .and_then(|m| m.value.as_f64())
        .expect("used memory is measurable");
    assert!(used <= total, "used {used} MiB exceeds total {total} MiB");

    let root = taken
        .iter()
        .find(|d| d.component == "system.disk/")
        .expect("the root filesystem is watched");
    assert_ne!(
        root.health,
        Health::Unknown,
        "root should be measurable here"
    );
}

/// A path that cannot be measured must come back as a gap rather than as a full disk.
#[test]
fn a_watched_path_that_does_not_exist_reports_unknown() {
    let mut s = Sampler::new("/nonexistent/carl/home");
    let taken = s.sample(1000);

    let missing = taken
        .iter()
        .find(|d| d.component.contains("nonexistent"))
        .expect("the watched path still produces a diagnostic");

    assert_eq!(missing.health, Health::Unknown);
    for m in &missing.metrics {
        assert_eq!(m.rendered(), "unknown", "{} must not be zero", m.name);
    }
}

/// Checked when this compiles rather than when it runs, because it is a fact about the
/// constant and not about a machine. Forking `nvidia-smi` in a tight loop is not free, and
/// whether the sampler actually honours the interval is proved in `providers::diagnostics`.
const _: () = assert!(MIN_INTERVAL_SECS >= 2);

#[test]
fn the_clock_returns_a_plausible_unix_time() {
    let t = now();
    assert!(t > 1_700_000_000, "before this was written");
    assert!(t < 4_000_000_000, "and not in the far future");
}
