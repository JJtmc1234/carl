//! The one thing an integrator calls to get everything.
//!
//! The two halves are kept apart everywhere else in this module and joined only here, at the
//! last moment, still labelled. `Snapshot` hands back two lists rather than one on purpose: a
//! caller that wants to draw them the same way has to do that deliberately rather than by not
//! noticing there was a difference.
//!
//! Sampling is rate limited here rather than left to the caller. A panel redrawing at sixty
//! frames a second must not fork `nvidia-smi` sixty times a second, and the cheapest place to
//! make that impossible is the place that owns the previous sample.

use std::path::{Path, PathBuf};

use super::army::Army;
use super::health::{Diagnostic, Health};
use super::system::{MIN_INTERVAL_SECS, Sampler, now, proc};

/// Everything the panel knows at one moment.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// Derived from the army's own records. True until something happens.
    pub army: Vec<Diagnostic>,
    /// Read off the machine. Stale immediately, and each carries when it was read.
    pub machine: Vec<Diagnostic>,
}

impl Snapshot {
    /// Everything, for a caller that genuinely wants one list.
    pub fn all(&self) -> Vec<&Diagnostic> {
        self.army.iter().chain(&self.machine).collect()
    }

    /// The worst thing in it, for a collapsed header.
    pub fn overall(&self) -> Health {
        Health::worst(self.all().into_iter().map(|d| d.health))
    }

    pub fn find(&self, component: &str) -> Option<&Diagnostic> {
        self.all().into_iter().find(|d| d.component == component)
    }
}

/// Reads the army and samples the machine.
pub struct Diagnostics {
    army: Army,
    sampler: Sampler,
    last_sample: Option<u64>,
    machine: Vec<Diagnostic>,
}

impl Diagnostics {
    /// `home` is Carl's data directory, normally `~/.carl`.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            sampler: Sampler::new(home.clone()),
            army: Army::new(home),
            last_sample: None,
            machine: Vec::new(),
        }
    }

    pub fn home(&self) -> &Path {
        self.army.home()
    }

    /// The army half, which costs a few file reads and can be called whenever something
    /// happened.
    pub fn army(&self) -> Vec<Diagnostic> {
        self.army.snapshot(machine_uptime())
    }

    /// The machine half, resampled only if enough time has passed.
    ///
    /// Returns the previous readings otherwise, unchanged and still carrying their original
    /// `measured_at`, so a caller polling quickly sees an honestly old number rather than a
    /// fresh looking one.
    pub fn machine(&mut self) -> Vec<Diagnostic> {
        self.machine_at(now())
    }

    /// The machine half as of a given moment.
    ///
    /// The clock is a parameter so the rate limit can be tested without waiting for real
    /// seconds to pass, which is the same convention the army modules use.
    pub fn machine_at(&mut self, at: u64) -> Vec<Diagnostic> {
        let due = self
            .last_sample
            .is_none_or(|last| at.saturating_sub(last) >= MIN_INTERVAL_SECS);

        if due {
            self.machine = self.sampler.sample(at);
            self.last_sample = Some(at);
        }
        self.machine.clone()
    }

    /// Both halves, ready to draw.
    pub fn snapshot(&mut self) -> Snapshot {
        Snapshot {
            army: self.army(),
            machine: self.machine(),
        }
    }

    /// Both halves as of a given moment.
    pub fn snapshot_at(&mut self, at: u64) -> Snapshot {
        Snapshot {
            army: self.army(),
            machine: self.machine_at(at),
        }
    }

    /// When the machine was last read, for a caller that wants to show the age itself.
    pub fn last_sampled_at(&self) -> Option<u64> {
        self.last_sample
    }
}

/// Seconds since boot, used to turn a service start stamp into an uptime.
fn machine_uptime() -> Option<f64> {
    proc::read(proc::UPTIME)
        .as_deref()
        .and_then(proc::parse_uptime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::health::Kind;

    #[test]
    fn a_snapshot_keeps_the_two_kinds_apart() {
        let d = tempfile::tempdir().unwrap();
        let mut diagnostics = Diagnostics::new(d.path());
        let taken = diagnostics.snapshot();

        assert!(!taken.army.is_empty());
        assert!(!taken.machine.is_empty());

        for a in &taken.army {
            assert_eq!(a.kind, Kind::EventDriven, "{}", a.component);
        }
        for m in &taken.machine {
            assert_eq!(m.kind, Kind::Sampled, "{}", m.component);
            assert!(m.measured_at.is_some(), "{}", m.component);
        }
    }

    /// A panel redrawing constantly must not fork a process every frame.
    #[test]
    fn sampling_is_rate_limited_and_says_so_honestly() {
        let d = tempfile::tempdir().unwrap();
        let mut diagnostics = Diagnostics::new(d.path());

        let first = diagnostics.machine_at(1_000);
        let again = diagnostics.machine_at(1_000 + MIN_INTERVAL_SECS - 1);

        assert_eq!(
            diagnostics.last_sampled_at(),
            Some(1_000),
            "a second call inside the interval must not resample"
        );
        assert_eq!(
            first.len(),
            again.len(),
            "and it returns the previous readings rather than nothing"
        );
        assert_eq!(
            again[0].measured_at, first[0].measured_at,
            "the age is the original age, not a fresh looking one"
        );

        // And once the interval has passed it does resample.
        diagnostics.machine_at(1_000 + MIN_INTERVAL_SECS);
        assert_eq!(
            diagnostics.last_sampled_at(),
            Some(1_000 + MIN_INTERVAL_SECS)
        );
    }

    #[test]
    fn the_army_half_can_be_read_as_often_as_something_happens() {
        let d = tempfile::tempdir().unwrap();
        let diagnostics = Diagnostics::new(d.path());
        // No rate limit and no state, so two reads in a row are both valid.
        assert_eq!(diagnostics.army().len(), diagnostics.army().len());
    }

    #[test]
    fn the_overall_health_is_the_worst_of_everything() {
        let d = tempfile::tempdir().unwrap();
        let mut diagnostics = Diagnostics::new(d.path());
        let taken = diagnostics.snapshot();

        let worst = Health::worst(taken.all().into_iter().map(|d| d.health));
        assert_eq!(taken.overall(), worst);
    }

    #[test]
    fn a_component_can_be_found_by_name() {
        let d = tempfile::tempdir().unwrap();
        let mut diagnostics = Diagnostics::new(d.path());
        let taken = diagnostics.snapshot();

        assert!(taken.find("system.memory").is_some());
        assert!(taken.find("army.journal").is_some());
        assert!(taken.find("nothing.like.this").is_none());
    }

    #[test]
    fn this_machine_reports_its_uptime() {
        let up = machine_uptime().expect("/proc/uptime is readable here");
        assert!(
            up > 0.0,
            "a machine that has been up no time at all is not running this"
        );
    }
}
