//! The one thing an integrator calls to get everything.
//!
//! Written for a caller that is a render loop, because that is what it will be. A panel drawing
//! at sixty frames a second will call this sixty times a second, and the difference between a
//! provider that survives that and one that does not is entirely in this file.
//!
//! Three costs, treated differently.
//!
//! **Records** are file reads. The journal and the agent folders. Cheap, never cached, always
//! current, because an event driven source that lags is not event driven.
//!
//! **Probes** start processes. Three `systemctl` calls and a walk of `/proc`. The meaning is
//! still event driven, the cost is not, so they are rate limited.
//!
//! **Samples** read the machine and fork `nvidia-smi`. Rate limited, and every reading carries
//! the moment it was taken, so a caller polling faster than the interval sees an honestly old
//! number rather than a fresh looking one.
//!
//! The two halves stay apart until the last moment. `Snapshot` hands back two lists, so a caller
//! that wants to draw them the same way has to decide that rather than not notice the
//! difference.

use std::path::{Path, PathBuf};

use super::army::Army;
use super::health::{Diagnostic, Health};
use super::system::{MIN_INTERVAL_SECS, Sampler, now, proc};

/// How often the expensive sources may run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intervals {
    /// Seconds between machine samples, which fork `nvidia-smi`.
    pub machine_secs: u64,
    /// Seconds between service and process probes, which fork `systemctl`.
    pub probe_secs: u64,
}

impl Default for Intervals {
    /// Slow enough that a render loop cannot hurt the machine, fast enough that a person does
    /// not notice the lag.
    fn default() -> Self {
        Self {
            machine_secs: MIN_INTERVAL_SECS,
            probe_secs: 5,
        }
    }
}

impl Intervals {
    /// Refuses an interval of zero, which would mean no rate limit at all.
    ///
    /// A caller wanting fresher numbers can ask for one second. A caller asking for zero has
    /// almost certainly not thought about the render loop, and the point of this type is that
    /// the thought is unavoidable.
    pub fn checked(self) -> Self {
        Self {
            machine_secs: self.machine_secs.max(1),
            probe_secs: self.probe_secs.max(1),
        }
    }
}

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

    /// One component by its id. The id is a lookup key and nothing else.
    pub fn find(&self, component: &str) -> Option<&Diagnostic> {
        self.all().into_iter().find(|d| d.component == component)
    }

    /// Every component id present, which is what a caller groups on.
    pub fn components(&self) -> Vec<&str> {
        self.all()
            .into_iter()
            .map(|d| d.component.as_str())
            .collect()
    }

    /// Any component id that appears more than once.
    ///
    /// Should always be empty. A duplicate means two sources are claiming the same identity and
    /// a panel keyed on the id would draw one on top of the other, so it is worth being able to
    /// ask rather than trusting that it cannot happen.
    pub fn duplicate_components(&self) -> Vec<String> {
        let mut seen: Vec<&str> = Vec::new();
        let mut twice: Vec<String> = Vec::new();
        for id in self.components() {
            if seen.contains(&id) {
                let owned = id.to_string();
                if !twice.contains(&owned) {
                    twice.push(owned);
                }
            } else {
                seen.push(id);
            }
        }
        twice
    }
}

/// What was last read, and when.
struct Cached {
    at: Option<u64>,
    value: Vec<Diagnostic>,
    /// How many times the underlying source actually ran, for proving the rate limit.
    runs: u64,
}

impl Cached {
    fn new() -> Self {
        Self {
            at: None,
            value: Vec::new(),
            runs: 0,
        }
    }

    fn due(&self, at: u64, every: u64) -> bool {
        self.at.is_none_or(|last| at.saturating_sub(last) >= every)
    }
}

/// Reads the army and samples the machine.
pub struct Diagnostics {
    army: Army,
    sampler: Sampler,
    intervals: Intervals,
    machine: Cached,
    probes: Cached,
}

impl Diagnostics {
    /// `home` is Carl's data directory, normally `~/.carl`.
    pub fn new(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        Self {
            sampler: Sampler::new(home.clone()),
            army: Army::new(home),
            intervals: Intervals::default(),
            machine: Cached::new(),
            probes: Cached::new(),
        }
    }

    /// Changes how often the expensive sources may run.
    pub fn every(mut self, intervals: Intervals) -> Self {
        self.intervals = intervals.checked();
        self
    }

    pub fn intervals(&self) -> Intervals {
        self.intervals
    }

    pub fn home(&self) -> &Path {
        self.army.home()
    }

    /// The cheap half of the army: journal and agent folders, always current.
    pub fn records(&self) -> Vec<Diagnostic> {
        self.army.records()
    }

    /// The army half: fresh records plus the last probe.
    pub fn army(&mut self) -> Vec<Diagnostic> {
        self.army_at(now())
    }

    /// The army half as of a given moment.
    pub fn army_at(&mut self, at: u64) -> Vec<Diagnostic> {
        let mut out = self.army.records();
        out.extend(self.probes_at(at));
        out
    }

    /// The service and process probe, rerun only if enough time has passed.
    pub fn probes_at(&mut self, at: u64) -> Vec<Diagnostic> {
        if self.probes.due(at, self.intervals.probe_secs) {
            self.probes.value = self.army.processes(machine_uptime());
            self.probes.at = Some(at);
            self.probes.runs += 1;
        }
        self.probes.value.clone()
    }

    /// The machine half, resampled only if enough time has passed.
    ///
    /// Returns the previous readings otherwise, unchanged and still carrying their original
    /// `measured_at`, so a caller polling quickly sees an honestly old number.
    pub fn machine(&mut self) -> Vec<Diagnostic> {
        self.machine_at(now())
    }

    /// The machine half as of a given moment.
    ///
    /// The clock is a parameter so the rate limit can be tested without waiting for real
    /// seconds to pass, which is the convention the army modules already use.
    pub fn machine_at(&mut self, at: u64) -> Vec<Diagnostic> {
        if self.machine.due(at, self.intervals.machine_secs) {
            self.machine.value = self.sampler.sample(at);
            self.machine.at = Some(at);
            self.machine.runs += 1;
        }
        self.machine.value.clone()
    }

    /// Both halves, ready to draw.
    pub fn snapshot(&mut self) -> Snapshot {
        self.snapshot_at(now())
    }

    /// Both halves as of a given moment.
    pub fn snapshot_at(&mut self, at: u64) -> Snapshot {
        Snapshot {
            army: self.army_at(at),
            machine: self.machine_at(at),
        }
    }

    /// When the machine was last read.
    pub fn last_sampled_at(&self) -> Option<u64> {
        self.machine.at
    }

    /// When the services were last probed.
    pub fn last_probed_at(&self) -> Option<u64> {
        self.probes.at
    }

    /// How many times the machine sampler has actually run.
    ///
    /// Exposed so a caller, or a test, can prove that driving this from a render loop does not
    /// start a process every frame.
    pub fn samples_taken(&self) -> u64 {
        self.machine.runs
    }

    /// How many times the service probe has actually run.
    pub fn probes_taken(&self) -> u64 {
        self.probes.runs
    }
}

/// Seconds since boot, used to turn a service start stamp into an uptime.
fn machine_uptime() -> Option<f64> {
    proc::read(proc::UPTIME)
        .as_deref()
        .and_then(proc::parse_uptime)
}

#[cfg(test)]
mod tests;
