//! The army, as it can be seen from outside the process running it.
//!
//! Everything here is `Kind::EventDriven`. It is read from files that change when something
//! happens rather than sampled on a clock, so it carries no `measured_at` and does not go
//! stale in the way a CPU reading does.
//!
//! Two rules shaped this module.
//!
//! **Nothing here is authoritative.** `army::task` owns task status and `army::personnel` owns
//! agent state. This reads what they wrote down and re-derives nothing. Where a judgement is
//! needed, the constant comes from the owning module, so `MAX_ATTEMPTS` is the same number in
//! both places by construction rather than by somebody remembering.
//!
//! **Reading must not write.** `Personnel::open` creates the army directory if it is not
//! there, which is correct for the thing that founds an army and quite wrong for a panel
//! looking at one. So the directory is checked first and an army that was never founded is
//! reported as exactly that, rather than being quietly brought into existence by being looked
//! at.

pub mod journal;
pub mod services;

use std::path::{Path, PathBuf};

use crate::army::personnel::Personnel;
use crate::providers::health::{Diagnostic, Health, Kind, Metric, Reading};
use crate::providers::system::process;

/// How stale the last journal entry may be before it is worth mentioning. Six hours.
const QUIET_JOURNAL_SECS: u64 = 6 * 60 * 60;

/// Reads the army from disk. Holds no state of its own.
pub struct Army {
    home: PathBuf,
}

impl Army {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn army_root(&self) -> PathBuf {
        self.home.join("army")
    }

    pub fn journal_path(&self) -> PathBuf {
        self.home.join("run").join("events.jsonl")
    }

    /// Whether an army has been founded here at all.
    pub fn founded(&self) -> bool {
        self.army_root().is_dir()
    }

    /// Every army diagnostic, read now.
    pub fn snapshot(&self, machine_uptime: Option<f64>) -> Vec<Diagnostic> {
        let mut out = services::diagnostics(machine_uptime);
        let folded = journal::fold_file(&self.journal_path());

        out.push(self.personnel());
        out.extend(self.agents());
        out.push(journal_health(&folded, &self.journal_path()));
        out.push(tasks(&folded));
        out.push(latency(&folded));
        out.push(claude_processes());
        out
    }

    /// The one line summary of the whole army, for a collapsed panel.
    pub fn overall(&self, machine_uptime: Option<f64>) -> Health {
        Health::worst(self.snapshot(machine_uptime).into_iter().map(|d| d.health))
    }

    fn personnel(&self) -> Diagnostic {
        if !self.founded() {
            return Diagnostic::new(
                "army.personnel",
                Health::Unknown,
                "no army has been founded in this home",
                Kind::EventDriven,
            )
            .with(Metric::unknown("agents", ""));
        }

        match Personnel::open(&self.home) {
            Err(e) => Diagnostic::new(
                "army.personnel",
                Health::Failed,
                format!("the folders will not load: {e}"),
                Kind::EventDriven,
            ),
            Ok(people) => {
                let missing = people.missing();
                // A folder that is absent is an agent with no state yet rather than a broken
                // organisation, because the hierarchy is compiled in and never lived on disk.
                let health = if missing.is_empty() {
                    Health::Healthy
                } else {
                    Health::Degraded
                };
                let summary = if missing.is_empty() {
                    format!("{} agents, all with folders", people.len())
                } else {
                    format!(
                        "{} agents, {} without a folder: {}",
                        people.len(),
                        missing.len(),
                        missing
                            .iter()
                            .map(|a| a.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                Diagnostic::new("army.personnel", health, summary, Kind::EventDriven)
                    .with(Metric::new("agents", Reading::Int(people.len() as u64), ""))
                    .with(Metric::new(
                        "without a folder",
                        Reading::Int(missing.len() as u64),
                        "",
                    ))
            }
        }
    }

    /// One diagnostic per agent that has a folder.
    fn agents(&self) -> Vec<Diagnostic> {
        if !self.founded() {
            return Vec::new();
        }
        let Ok(people) = Personnel::open(&self.home) else {
            return Vec::new();
        };

        people
            .names()
            .into_iter()
            .map(|name| {
                let state = people.state(name);
                let holding = state.and_then(|s| s.holding.as_ref());
                let summary = match holding {
                    Some(task) => format!("holding {task}"),
                    None => "idle".to_string(),
                };
                Diagnostic::new(
                    &format!("agent.{name}"),
                    Health::Healthy,
                    summary,
                    Kind::EventDriven,
                )
                .with(Metric::new(
                    "holding",
                    holding.map_or(Reading::Text("nothing".into()), |t| {
                        Reading::Text(t.to_string())
                    }),
                    "",
                ))
                .with(Metric::new(
                    "last note",
                    state
                        .and_then(|s| s.recent.last())
                        .map_or(Reading::Unknown, |n| Reading::Text(n.clone())),
                    "",
                ))
            })
            .collect()
    }
}

fn journal_health(folded: &journal::Folded, path: &Path) -> Diagnostic {
    if !path.exists() {
        return Diagnostic::new(
            "army.journal",
            Health::Unknown,
            "no journal has been written yet",
            Kind::EventDriven,
        )
        .with(Metric::unknown("events", ""));
    }

    // A hole in the record is a real fault. The journal is the army's memory and a line that
    // cannot be read is a thing that happened and can no longer be counted.
    let health = if folded.unreadable_lines > 0 {
        Health::Degraded
    } else {
        Health::Healthy
    };
    let summary = if folded.unreadable_lines > 0 {
        format!(
            "{} events, {} unreadable lines",
            folded.records, folded.unreadable_lines
        )
    } else {
        format!("{} events", folded.records)
    };

    Diagnostic::new("army.journal", health, summary, Kind::EventDriven)
        .with(Metric::new(
            "events",
            Reading::Int(folded.records as u64),
            "",
        ))
        .with(Metric::new(
            "unreadable lines",
            Reading::Int(folded.unreadable_lines as u64),
            "",
        ))
        .with(Metric::new(
            "last sequence",
            folded.last_seq.map_or(Reading::Unknown, Reading::Int),
            "",
        ))
        .with(Metric::new(
            "last event",
            folded.last_at.map_or(Reading::Unknown, Reading::Int),
            "",
        ))
}

fn tasks(folded: &journal::Folded) -> Diagnostic {
    let active = folded.active().len();
    let blocked = folded.blocked().len();
    let failed = folded.failed().len();

    // Blocked outranks failed here on purpose. An abandoned task is over and needs nobody,
    // while a blocked one is waiting on a person and is the thing worth looking at.
    let health = match (blocked, failed, active) {
        (0, 0, _) => Health::Healthy,
        (0, _, _) => Health::Failed,
        _ => Health::Blocked,
    };

    let summary = match (active, blocked) {
        (0, _) => "nothing in hand".to_string(),
        (a, 0) => format!("{a} in hand"),
        (a, b) => format!("{a} in hand, {b} blocked"),
    };

    Diagnostic::new("army.tasks", health, summary, Kind::EventDriven)
        .with(Metric::new("in hand", Reading::Int(active as u64), ""))
        .with(Metric::new("blocked", Reading::Int(blocked as u64), ""))
        .with(Metric::new("abandoned", Reading::Int(failed as u64), ""))
        .with(Metric::new(
            "refusals",
            Reading::Int(folded.refusals.len() as u64),
            "",
        ))
}

fn latency(folded: &journal::Folded) -> Diagnostic {
    let mut spans = folded.handback_seconds();
    spans.sort_unstable();

    // No finished handover means no latency to report. Zero would claim instant work.
    let (health, summary, typical, worst) = match spans.as_slice() {
        [] => (
            Health::Unknown,
            "no completed handovers to measure".to_string(),
            Reading::Unknown,
            Reading::Unknown,
        ),
        sorted => {
            let median = sorted[sorted.len() / 2];
            let max = *sorted.last().unwrap_or(&0);
            (
                Health::Healthy,
                format!("median handover {median}s over {} tasks", sorted.len()),
                Reading::Int(median),
                Reading::Int(max),
            )
        }
    };

    Diagnostic::new("army.latency", health, summary, Kind::EventDriven)
        .with(Metric::new("median handover", typical, "s"))
        .with(Metric::new("slowest handover", worst, "s"))
        .with(Metric::new(
            "measured tasks",
            Reading::Int(spans.len() as u64),
            "",
        ))
}

/// How many `claude` processes are alive, read from `/proc`.
///
/// There is no cross process registry of Claude sessions. `Pool::open_count` knows, but only
/// inside the process that owns the pool, and the panel is not that process. Counting them in
/// `/proc` is the only answer available from outside and is labelled as a count of processes
/// rather than of sessions, because one is not the other.
fn claude_processes() -> Diagnostic {
    let found = process::find_by_name("claude");
    let zombies = found.iter().filter(|p| p.is_zombie()).count();
    let resident: u64 = found.iter().filter_map(|p| p.resident_kb).sum();

    // A zombie is a finished process nobody reaped, which means whoever started it stopped
    // watching. That is worth flagging even though it costs almost nothing.
    let health = if zombies > 0 {
        Health::Degraded
    } else {
        Health::Healthy
    };
    let summary = match (found.len(), zombies) {
        (0, _) => "no claude processes".to_string(),
        (n, 0) => format!("{n} claude processes"),
        (n, z) => format!("{n} claude processes, {z} unreaped"),
    };

    Diagnostic::new("claude.processes", health, summary, Kind::EventDriven)
        .with(Metric::new(
            "processes",
            Reading::Int(found.len() as u64),
            "",
        ))
        .with(Metric::new("unreaped", Reading::Int(zombies as u64), ""))
        .with(Metric::new(
            "resident",
            if found.is_empty() {
                Reading::Unknown
            } else {
                Reading::Int(resident / 1024)
            },
            " MiB",
        ))
}

/// Whether the journal has been quiet for long enough to be worth a look.
pub fn journal_is_quiet(folded: &journal::Folded, now: u64) -> bool {
    folded
        .last_at
        .is_some_and(|at| now.saturating_sub(at) > QUIET_JOURNAL_SECS)
}

#[cfg(test)]
mod tests;
