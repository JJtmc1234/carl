//! The machine, sampled.
//!
//! Everything in here is `Kind::Sampled`. It is a number somebody read at a moment and it is
//! stale the instant it is taken, which is why every diagnostic carries `measured_at` and why
//! the panel is expected to show its age. Army state is the opposite and lives in `army.rs`.
//!
//! The first sample cannot report CPU utilisation, because utilisation is a difference between
//! two readings and there is only one. It comes back unknown rather than zero. That is the
//! whole discipline of this module in one case: an idle machine and an unmeasured machine must
//! never render the same.

pub mod disk;
pub mod gpu;
pub mod proc;
pub mod process;
pub mod started;

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::health::{Diagnostic, Health, Kind, Metric, Reading};

/// The fastest anything here should be asked for.
///
/// Two seconds. Faster buys nothing a person can read, and the GPU sample forks a process.
pub const MIN_INTERVAL_SECS: u64 = 2;

/// Unix seconds now.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Holds what the last sample saw, because the interesting numbers are differences.
pub struct Sampler {
    previous_cpu: Option<proc::CpuTimes>,
    /// Filesystems worth watching. Kept small on purpose.
    watched: Vec<PathBuf>,
}

impl Sampler {
    /// Watches the root filesystem and wherever Carl keeps its data.
    ///
    /// The two are resolved and deduplicated, because a home of `/` would otherwise produce
    /// two diagnostics with the same component id and the panel groups on that id. A path that
    /// does not exist cannot be resolved and is kept as written, so it still produces an
    /// honest unknown rather than disappearing.
    pub fn new(carl_home: impl Into<PathBuf>) -> Self {
        let mut watched: Vec<PathBuf> = Vec::new();
        for path in [PathBuf::from("/"), carl_home.into()] {
            let resolved = path.canonicalize().unwrap_or(path);
            if !watched.contains(&resolved) {
                watched.push(resolved);
            }
        }
        Self {
            previous_cpu: None,
            watched,
        }
    }

    /// Every machine diagnostic, taken now.
    pub fn sample(&mut self, at: u64) -> Vec<Diagnostic> {
        let mut out = vec![self.cpu(at), memory(at)];
        out.extend(self.watched.iter().map(|p| disk_diagnostic(p, at)));
        out.push(graphics(at));
        out.push(temperature(at));
        out.push(network(at));
        out
    }

    fn cpu(&mut self, at: u64) -> Diagnostic {
        let times = proc::read(proc::STAT)
            .as_deref()
            .and_then(proc::parse_cpu_times);
        let busy = match (times, self.previous_cpu) {
            (Some(now), Some(before)) => now.busy_fraction_since(&before),
            _ => None,
        };
        if let Some(t) = times {
            self.previous_cpu = Some(t);
        }

        let load = proc::read(proc::LOADAVG)
            .as_deref()
            .and_then(proc::parse_loadavg);
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .ok();

        // Load is judged against core count, because 5.0 is calm on sixteen cores and dire on
        // two. Utilisation alone cannot say that.
        let health = match (busy, load, cores) {
            (_, Some(l), Some(c)) if l[0] > c * 1.5 => Health::Degraded,
            (Some(b), _, _) if b > 0.95 => Health::Degraded,
            (None, None, _) => Health::Unknown,
            _ => Health::Healthy,
        };

        let summary = match busy {
            Some(b) => format!("{:.0}% busy", b * 100.0),
            None => "utilisation needs two samples".to_string(),
        };

        Diagnostic::new("system.cpu", health, summary, Kind::Sampled)
            .measured(at)
            .with(Metric::new(
                "utilisation",
                busy.map_or(Reading::Unknown, |b| Reading::Float(b * 100.0)),
                "%",
            ))
            .with(Metric::new(
                "load 1m",
                load.map_or(Reading::Unknown, |l| Reading::Float(l[0])),
                "",
            ))
            .with(Metric::new(
                "cores",
                cores.map_or(Reading::Unknown, |c| Reading::Int(c as u64)),
                "",
            ))
    }
}

fn memory(at: u64) -> Diagnostic {
    let info = proc::read(proc::MEMINFO)
        .as_deref()
        .map(proc::parse_meminfo)
        .unwrap_or_default();

    let used = info.used_fraction();
    let health = match used {
        Some(f) if f > 0.95 => Health::Degraded,
        Some(_) => Health::Healthy,
        None => Health::Unknown,
    };
    let summary = match used {
        Some(f) => format!("{:.0}% of memory in use", f * 100.0),
        None => "memory could not be read".to_string(),
    };

    let mib = |kb: Option<u64>| kb.map_or(Reading::Unknown, |v| Reading::Int(v / 1024));

    Diagnostic::new("system.memory", health, summary, Kind::Sampled)
        .measured(at)
        .with(Metric::new("used", mib(info.used_kb()), " MiB"))
        .with(Metric::new("total", mib(info.total_kb), " MiB"))
        .with(Metric::new("swap used", mib(info.swap_used_kb()), " MiB"))
        .with(Metric::new("swap total", mib(info.swap_total_kb), " MiB"))
}

fn disk_diagnostic(path: &std::path::Path, at: u64) -> Diagnostic {
    // A colon between the family and the path, so the id is unambiguous and stable.
    let component = format!("system.disk:{}", path.display());
    let space = disk::space_for(path);

    // A full disk is a failure rather than a warning. Carl cannot append to the journal, and a
    // journal that cannot be appended to is the army losing its memory.
    let health = match space.and_then(|s| s.used_fraction()) {
        Some(f) if f >= 0.95 => Health::Failed,
        Some(f) if f >= 0.85 => Health::Degraded,
        Some(_) => Health::Healthy,
        None => Health::Unknown,
    };
    let summary = match space.and_then(|s| s.used_fraction()) {
        Some(f) => format!("{:.0}% full", f * 100.0),
        None => format!("{} could not be measured", path.display()),
    };

    let gib = |b: Option<u64>| b.map_or(Reading::Unknown, |v| Reading::Int(v / (1 << 30)));

    Diagnostic::new(&component, health, summary, Kind::Sampled)
        .measured(at)
        .with(Metric::new(
            "free",
            gib(space.map(|s| s.available_bytes)),
            " GiB",
        ))
        .with(Metric::new(
            "total",
            gib(space.map(|s| s.total_bytes)),
            " GiB",
        ))
}

fn graphics(at: u64) -> Diagnostic {
    let Some(card) = gpu::read_gpu() else {
        return Diagnostic::new(
            "system.gpu",
            Health::Unknown,
            "no NVIDIA card answered",
            Kind::Sampled,
        )
        .measured(at)
        .with(Metric::unknown("utilisation", "%"))
        .with(Metric::unknown("memory used", " MiB"));
    };

    let summary = match card.utilisation_percent {
        Some(u) => format!("{} at {u:.0}%", card.name),
        None => card.name.clone(),
    };

    Diagnostic::new("system.gpu", Health::Healthy, summary, Kind::Sampled)
        .measured(at)
        .with(Metric::new(
            "utilisation",
            card.utilisation_percent
                .map_or(Reading::Unknown, Reading::Float),
            "%",
        ))
        .with(Metric::new(
            "memory used",
            card.memory_used_mib.map_or(Reading::Unknown, Reading::Int),
            " MiB",
        ))
        .with(Metric::new(
            "memory total",
            card.memory_total_mib.map_or(Reading::Unknown, Reading::Int),
            " MiB",
        ))
        .with(Metric::new(
            "temperature",
            card.temperature_c.map_or(Reading::Unknown, Reading::Float),
            "C",
        ))
}

fn temperature(at: u64) -> Diagnostic {
    let cpu = gpu::read_cpu_temperature();
    let health = match cpu {
        Some(c) if c >= 95.0 => Health::Failed,
        Some(c) if c >= 85.0 => Health::Degraded,
        Some(_) => Health::Healthy,
        None => Health::Unknown,
    };
    let summary = match cpu {
        Some(c) => format!("CPU package at {c:.0}C"),
        None => "no readable thermal zone".to_string(),
    };

    Diagnostic::new("system.temperature", health, summary, Kind::Sampled)
        .measured(at)
        .with(Metric::new(
            "cpu package",
            cpu.map_or(Reading::Unknown, Reading::Float),
            "C",
        ))
}

fn network(at: u64) -> Diagnostic {
    let totals = proc::read(proc::NET_DEV)
        .as_deref()
        .and_then(proc::parse_net_dev);

    let (health, summary) = match totals {
        Some(_) => (Health::Healthy, "interfaces counting".to_string()),
        None => (Health::Unknown, "no interface counters".to_string()),
    };

    let mib = |b: Option<u64>| b.map_or(Reading::Unknown, |v| Reading::Int(v / (1 << 20)));

    Diagnostic::new("system.network", health, summary, Kind::Sampled)
        .measured(at)
        .with(Metric::new("received", mib(totals.map(|t| t.0)), " MiB"))
        .with(Metric::new("sent", mib(totals.map(|t| t.1)), " MiB"))
}

#[cfg(test)]
mod tests;
