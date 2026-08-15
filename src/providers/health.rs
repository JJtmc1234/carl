//! What a reading is worth, and how sure we are of it.
//!
//! Two ideas carry this file, and both exist because the panel has to be able to tell the
//! truth about its own ignorance.
//!
//! **Unknown is a value.** A metric nobody could measure is `Reading::Unknown`, never zero and
//! never an empty string. Zero disk free and unmeasurable disk free look identical once you
//! flatten them, and one of those is an emergency.
//!
//! **Sampled is not the same as event driven.** Army state comes from the journal and the task
//! records, and it is true until something changes it. Machine telemetry is a number somebody
//! read off `/proc` at a moment, and it is stale immediately. Anything that mixes them ends up
//! showing a two second old CPU figure with the same confidence as a task that is definitely
//! blocked.

use std::fmt;

/// How a component is doing.
///
/// Five states rather than two. Green and red cannot express "the agent is alive and waiting
/// on somebody", which is most of what a chain of command does all day.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// Working as intended.
    Healthy,
    /// Working, but worse than it should be. Still making progress.
    Degraded,
    /// Not making progress and waiting on somebody. Nothing is broken.
    Blocked,
    /// Stopped, crashed, or refused to do what it was for.
    Failed,
    /// Could not be measured. Not a judgement about the component at all.
    Unknown,
}

impl Health {
    /// The worst of a set, which is how a parent summarises its children.
    ///
    /// `Unknown` deliberately does not win. A component with one unmeasurable metric and three
    /// healthy ones is better described as healthy with a gap than as a mystery, and treating
    /// ignorance as the worst case makes the whole panel go grey the moment one file is
    /// unreadable.
    pub fn worst(of: impl IntoIterator<Item = Health>) -> Health {
        let mut seen_any = false;
        let mut worst = Health::Healthy;
        let mut all_unknown = true;

        for h in of {
            seen_any = true;
            if h != Health::Unknown {
                all_unknown = false;
                if h > worst && h != Health::Unknown {
                    worst = h;
                }
            }
        }

        match (seen_any, all_unknown) {
            (false, _) => Health::Unknown,
            (true, true) => Health::Unknown,
            _ => worst,
        }
    }
}

impl fmt::Display for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Health::Healthy => "healthy",
            Health::Degraded => "degraded",
            Health::Blocked => "blocked",
            Health::Failed => "failed",
            Health::Unknown => "unknown",
        })
    }
}

/// Where a reading came from, which decides how the panel is allowed to present it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Read off the machine at a moment. Carries a `measured_at` and goes stale.
    Sampled,
    /// Derived from army state that changes only when something happens.
    EventDriven,
}

/// One measured number, or an honest admission that there is not one.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reading {
    Int(u64),
    Float(f64),
    Text(String),
    /// The source was missing, unreadable, or did not apply to this machine.
    Unknown,
}

impl Reading {
    pub fn is_known(&self) -> bool {
        !matches!(self, Reading::Unknown)
    }

    /// The number, when there is one and it is a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Reading::Int(n) => Some(*n as f64),
            Reading::Float(f) => Some(*f),
            _ => None,
        }
    }
}

impl fmt::Display for Reading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Reading::Int(n) => write!(f, "{n}"),
            Reading::Float(v) => write!(f, "{v:.1}"),
            Reading::Text(s) => f.write_str(s),
            Reading::Unknown => f.write_str("unknown"),
        }
    }
}

/// One named measurement, with its unit kept separate so the panel can format it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Metric {
    pub name: String,
    pub value: Reading,
    /// A bare unit such as `%`, `MiB`, `s`. Empty when the number is a count.
    pub unit: String,
}

impl Metric {
    pub fn new(name: &str, value: Reading, unit: &str) -> Self {
        Self {
            name: name.to_string(),
            value,
            unit: unit.to_string(),
        }
    }

    /// A metric nobody could read. The unit is kept, because what it would have been is still
    /// useful to whoever is looking at the gap.
    pub fn unknown(name: &str, unit: &str) -> Self {
        Self::new(name, Reading::Unknown, unit)
    }

    /// The number and its unit, rendered.
    pub fn rendered(&self) -> String {
        match (&self.value, self.unit.as_str()) {
            (Reading::Unknown, _) => "unknown".to_string(),
            (v, "") => v.to_string(),
            (v, u) => format!("{v}{u}"),
        }
    }
}

/// One component's condition, which is the unit the panel draws.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Diagnostic {
    /// What this is about, for example `carl.service` or `agent.nora`.
    pub component: String,
    pub health: Health,
    /// One line a person can read without expanding anything.
    pub summary: String,
    pub metrics: Vec<Metric>,
    /// Unix seconds the measurement was taken. Always present for `Sampled` and normally
    /// absent for `EventDriven`, which is true until something changes it.
    pub measured_at: Option<u64>,
    pub kind: Kind,
}

impl Diagnostic {
    pub fn new(component: &str, health: Health, summary: impl Into<String>, kind: Kind) -> Self {
        Self {
            component: component.to_string(),
            health,
            summary: summary.into(),
            metrics: Vec::new(),
            measured_at: None,
            kind,
        }
    }

    pub fn with(mut self, metric: Metric) -> Self {
        self.metrics.push(metric);
        self
    }

    pub fn measured(mut self, at: u64) -> Self {
        self.measured_at = Some(at);
        self
    }

    /// Which board this belongs on, taken from the component id.
    ///
    /// The convention is two families and only two: `army.*` and `system.*`. Everything else
    /// is a mistake, and `unknown` is returned rather than guessing, so a stray id shows up as
    /// a stray rather than being quietly filed under one of them.
    pub fn group(&self) -> &str {
        match self.component.split_once('.') {
            Some(("army", _)) => "army",
            Some(("system", _)) => "system",
            _ => "unknown",
        }
    }

    /// A deliberately lossy view, for a caller whose model has no room for a missing value.
    ///
    /// This is offered rather than imposed. The canonical `Diagnostic` keeps everything: a
    /// sampled reading that could not be taken still knows *when* the attempt was made, and
    /// still names the metrics that were missing. Both facts are thrown away here, so nothing
    /// downstream of this can recover them. Call it at the edge, never in a collector.
    pub fn flattened(&self) -> Flattened {
        let unreadable = self.health == Health::Unknown;
        Flattened {
            component: self.component.clone(),
            group: self.group().to_string(),
            health: self.health,
            summary: self.summary.clone(),
            // A component nobody could read becomes a gap: no age, no metrics.
            measured_at: if unreadable { None } else { self.measured_at },
            metrics: if unreadable {
                Vec::new()
            } else {
                self.metric_pairs()
            },
            kind: self.kind,
        }
    }

    /// The metrics as flat name and value pairs.
    ///
    /// For a caller that wants to render without knowing about units or `Reading`. An unknown
    /// value comes out as the word unknown rather than as an empty string, so a consumer that
    /// only ever sees this form still cannot mistake it for zero.
    pub fn metric_pairs(&self) -> Vec<(String, String)> {
        self.metrics
            .iter()
            .map(|m| (m.name.clone(), m.rendered()))
            .collect()
    }
}

/// A `Diagnostic` with the detail of an unreadable component removed.
///
/// Produced only by `Diagnostic::flattened`, never stored, never returned by a collector.
#[derive(Debug, Clone, PartialEq)]
pub struct Flattened {
    pub component: String,
    /// `army`, `system`, or `unknown`.
    pub group: String,
    pub health: Health,
    pub summary: String,
    pub metrics: Vec<(String, String)>,
    pub measured_at: Option<u64>,
    pub kind: Kind,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_worst_of_several_is_what_a_parent_reports() {
        use Health::*;
        assert_eq!(Health::worst([Healthy, Healthy]), Healthy);
        assert_eq!(Health::worst([Healthy, Degraded]), Degraded);
        assert_eq!(Health::worst([Degraded, Blocked]), Blocked);
        assert_eq!(Health::worst([Blocked, Failed]), Failed);
        assert_eq!(Health::worst([Healthy, Failed, Degraded]), Failed);
    }

    /// One unreadable file should not turn a working component into a mystery.
    #[test]
    fn one_unknown_among_known_values_does_not_win() {
        use Health::*;
        assert_eq!(Health::worst([Healthy, Unknown]), Healthy);
        assert_eq!(Health::worst([Unknown, Degraded]), Degraded);
    }

    /// But nothing measurable at all is genuinely unknown, and saying healthy there would be
    /// a lie.
    #[test]
    fn nothing_measurable_is_unknown_rather_than_healthy() {
        assert_eq!(
            Health::worst([Health::Unknown, Health::Unknown]),
            Health::Unknown
        );
        assert_eq!(Health::worst([]), Health::Unknown);
    }

    /// The rule the whole file exists for.
    #[test]
    fn an_unmeasurable_metric_never_renders_as_zero() {
        let m = Metric::unknown("disk free", "GiB");
        assert_eq!(m.rendered(), "unknown");
        assert!(!m.rendered().contains('0'));
        assert!(!m.value.is_known());
        assert_eq!(m.value.as_f64(), None);
    }

    #[test]
    fn a_known_metric_renders_with_its_unit() {
        assert_eq!(
            Metric::new("cpu", Reading::Float(12.34), "%").rendered(),
            "12.3%"
        );
        assert_eq!(Metric::new("agents", Reading::Int(4), "").rendered(), "4");
        assert_eq!(
            Metric::new("model", Reading::Text("claude-opus-5".into()), "").rendered(),
            "claude-opus-5"
        );
    }

    #[test]
    fn flat_pairs_keep_the_word_unknown() {
        let d = Diagnostic::new("disk", Health::Unknown, "no source", Kind::Sampled)
            .with(Metric::unknown("free", "GiB"))
            .with(Metric::new("total", Reading::Int(512), "GiB"));
        let pairs = d.metric_pairs();
        assert_eq!(pairs[0], ("free".to_string(), "unknown".to_string()));
        assert_eq!(pairs[1], ("total".to_string(), "512GiB".to_string()));
    }

    #[test]
    fn a_component_id_says_which_board_it_belongs_on() {
        let army = Diagnostic::new("army.tasks", Health::Healthy, "fine", Kind::EventDriven);
        assert_eq!(army.group(), "army");

        let system = Diagnostic::new("system.cpu", Health::Healthy, "fine", Kind::Sampled);
        assert_eq!(system.group(), "system");

        // A stray id is reported as stray rather than filed under a guess.
        let stray = Diagnostic::new("weather.outside", Health::Healthy, "mild", Kind::Sampled);
        assert_eq!(stray.group(), "unknown");
        let bare = Diagnostic::new("nodots", Health::Healthy, "x", Kind::Sampled);
        assert_eq!(bare.group(), "unknown");
    }

    /// The lossy view is available and does not change the value it came from.
    #[test]
    fn flattening_an_unreadable_component_drops_detail_without_destroying_it() {
        let rich = Diagnostic::new("system.gpu", Health::Unknown, "no card", Kind::Sampled)
            .measured(1_000)
            .with(Metric::unknown("utilisation", "%"));

        let flat = rich.flattened();
        assert_eq!(flat.measured_at, None, "the caller asked for a gap");
        assert!(flat.metrics.is_empty());
        assert_eq!(flat.group, "system");

        // The canonical value still knows both things, which is the whole point.
        assert_eq!(
            rich.measured_at,
            Some(1_000),
            "we did look, at a known moment"
        );
        assert_eq!(rich.metrics.len(), 1, "and we still know what was missing");
        assert_eq!(rich.metrics[0].rendered(), "unknown");
    }

    #[test]
    fn flattening_a_readable_component_keeps_everything() {
        let rich = Diagnostic::new("system.cpu", Health::Healthy, "31% busy", Kind::Sampled)
            .measured(1_000)
            .with(Metric::new("utilisation", Reading::Float(31.0), "%"));

        let flat = rich.flattened();
        assert_eq!(flat.measured_at, Some(1_000));
        assert_eq!(flat.metrics.len(), 1);
        assert_eq!(flat.metrics[0].1, "31.0%");
    }

    #[test]
    fn sampled_readings_carry_when_they_were_taken() {
        let d = Diagnostic::new("cpu", Health::Healthy, "fine", Kind::Sampled).measured(1000);
        assert_eq!(d.measured_at, Some(1000));
        let e = Diagnostic::new("task", Health::Blocked, "waiting", Kind::EventDriven);
        assert_eq!(
            e.measured_at, None,
            "event driven state is true until it changes"
        );
    }
}
