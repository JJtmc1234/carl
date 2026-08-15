//! One reading about one component.
//!
//! Process 3 collects these. The panel only draws them, and the two rules it draws by are worth
//! stating because they are what stop a diagnostics screen becoming decoration.
//!
//! **Unknown is a state, not a zero.** A collector that could not read a temperature returns
//! `Health::Unknown` and no metrics, and the panel shows a gap. Filling it with 0 would be a
//! number JJ could act on, and acting on it would be acting on nothing.
//!
//! **Sampled and event driven are different things.** A CPU figure is true at the instant it
//! was measured and is stale a second later. A blocked task is true until something changes it.
//! Showing both with the same freshness cue tells you one of them is lying, so they are
//! separate and drawn differently: sampled carries the age of the reading, event driven does
//! not, because its age is not the point.

/// How healthy a component is.
///
/// Ordered worst first, so sorting a list puts what needs attention at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Health {
    Failed,
    Blocked,
    Degraded,
    Unknown,
    Healthy,
}

impl Health {
    pub fn label(self) -> &'static str {
        match self {
            Health::Failed => "FAILED",
            Health::Blocked => "BLOCKED",
            Health::Degraded => "DEGRADED",
            Health::Unknown => "UNKNOWN",
            Health::Healthy => "HEALTHY",
        }
    }

    /// Whether this should pull the eye.
    ///
    /// Unknown deliberately does not. It is a gap in what we measured rather than a fault, and
    /// treating every unmeasured thing as an alarm is how a panel trains somebody to ignore it.
    pub fn wants_attention(self) -> bool {
        matches!(self, Health::Failed | Health::Blocked | Health::Degraded)
    }
}

/// Whether a reading is a sample or a state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reading {
    /// Measured at an instant and stale immediately. Shown with its age.
    Sampled,
    /// True until something changes it. Its age is not interesting.
    EventDriven,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// What this is about, like "carl.journal" or "system.cpu".
    pub component: String,
    /// Which board it belongs on, "army" or "system".
    pub group: String,
    pub health: Health,
    pub summary: String,
    /// Name and already formatted value. The panel does not do unit maths on somebody else's
    /// numbers, because guessing at a unit is how a percentage becomes a temperature.
    pub metrics: Vec<(String, String)>,
    pub reading: Reading,
    /// Unix seconds. `None` when nothing has ever measured it.
    pub measured_at: Option<u64>,
}

impl Diagnostic {
    /// A component that exists and has never been read.
    pub fn unknown(component: &str, group: &str) -> Self {
        Self {
            component: component.to_string(),
            group: group.to_string(),
            health: Health::Unknown,
            summary: "no reading".into(),
            metrics: Vec::new(),
            reading: Reading::Sampled,
            measured_at: None,
        }
    }

    /// How old the reading is, when that means anything.
    ///
    /// `None` for an event driven state, because it is current until it changes and showing an
    /// age next to it would suggest it decays.
    pub fn age_secs(&self, now: u64) -> Option<u64> {
        match self.reading {
            Reading::EventDriven => None,
            Reading::Sampled => self.measured_at.map(|at| now.saturating_sub(at)),
        }
    }

    /// Whether a sampled reading is old enough that it should stop being presented as current.
    pub fn stale(&self, now: u64, limit: u64) -> bool {
        self.age_secs(now).is_some_and(|age| age > limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sorting a board must put what is broken at the top and healthy at the bottom.
    #[test]
    fn worse_health_sorts_first() {
        let mut all = vec![
            Health::Healthy,
            Health::Unknown,
            Health::Failed,
            Health::Degraded,
            Health::Blocked,
        ];
        all.sort();
        assert_eq!(
            all,
            vec![
                Health::Failed,
                Health::Blocked,
                Health::Degraded,
                Health::Unknown,
                Health::Healthy
            ]
        );
    }

    /// A gap in measurement is not a fault. Treating it as one trains somebody to ignore the
    /// screen, which costs more than the missing reading did.
    #[test]
    fn unknown_is_not_an_alarm() {
        assert!(!Health::Unknown.wants_attention());
        assert!(!Health::Healthy.wants_attention());
        for bad in [Health::Failed, Health::Blocked, Health::Degraded] {
            assert!(bad.wants_attention(), "{bad:?}");
        }
    }

    /// An event driven state is current until it changes, so putting an age beside it would
    /// suggest it decays when it does not.
    #[test]
    fn only_a_sampled_reading_has_an_age() {
        let mut d = Diagnostic::unknown("system.cpu", "system");
        d.measured_at = Some(100);
        d.reading = Reading::Sampled;
        assert_eq!(d.age_secs(160), Some(60));

        d.reading = Reading::EventDriven;
        assert_eq!(d.age_secs(160), None, "a state does not age");
        assert!(!d.stale(9_999, 5), "and can never be stale");
    }

    /// Nothing has measured it, so there is no age to show and nothing to invent.
    #[test]
    fn a_component_nobody_measured_has_no_reading_at_all() {
        let d = Diagnostic::unknown("system.gpu", "system");
        assert_eq!(d.health, Health::Unknown);
        assert_eq!(d.age_secs(500), None);
        assert!(d.metrics.is_empty(), "no invented zeroes");
        assert!(!d.stale(500, 5), "unmeasured is not stale, it is absent");
    }

    #[test]
    fn a_sampled_reading_goes_stale() {
        let mut d = Diagnostic::unknown("system.cpu", "system");
        d.measured_at = Some(100);
        assert!(!d.stale(102, 5));
        assert!(d.stale(120, 5));
    }
}
