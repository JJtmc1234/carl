//! The few things the screen needs to know that the canonical types do not carry.
//!
//! Deliberately tiny. Everything here is a question about drawing rather than about the army,
//! which is why it lives on this side of the boundary. If any of it ever becomes a fact the
//! collectors know, it moves out of here and this file shrinks.

use carl::providers::health::{Diagnostic, Kind};

/// How old a sampled reading may be before it stops being shown as current.
pub const STALE_AFTER: u64 = 30;

/// Which board a component belongs on.
///
/// One rule, and it survives the rename Process 3 is making. The machine board is `system.`
/// and the army board is everything else, so `army.agent.nora` and the older `agent.nora` both
/// land in the right place and nothing needs a list of legacy prefixes kept up to date.
///
/// This is a question about layout rather than about health, which is why the collector does
/// not answer it. When `Diagnostic::group()` lands this becomes a call to it.
pub fn group_of(component: &str) -> &'static str {
    match component.split(['.', ':']).next() {
        Some("system") => "system",
        _ => "army",
    }
}

/// How old a reading is, when age means anything.
///
/// `None` for event driven state, which is true until something changes it. Putting a clock
/// beside it would suggest it decays, and implying army state is going stale tells somebody to
/// distrust a fact that is still perfectly true.
pub fn age_secs(d: &Diagnostic, now: u64) -> Option<u64> {
    match d.kind {
        Kind::EventDriven => None,
        Kind::Sampled => d.measured_at.map(|at| now.saturating_sub(at)),
    }
}

/// Whether a sampled reading is too old to present as current.
pub fn stale(d: &Diagnostic, now: u64) -> bool {
    age_secs(d, now).is_some_and(|age| age > STALE_AFTER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use carl::providers::health::Health;

    /// The rule has to work for the ids on main today and the ones Process 3 is renaming to,
    /// without a list of legacy prefixes to keep up to date.
    #[test]
    fn the_board_is_decided_by_one_rule_that_survives_the_rename() {
        assert_eq!(group_of("system.cpu"), "system");
        assert_eq!(group_of("army.agent.nora"), "army");
        assert_eq!(group_of("army.service.carl-slack"), "army");
        assert_eq!(group_of("system.disk:/"), "system");

        // The ids on main today, which must still land on the right board.
        assert_eq!(group_of("carl.service"), "army");
        assert_eq!(group_of("agent.nora"), "army");
        assert_eq!(group_of("claude.processes"), "army");

        // And anything unexpected goes to the army board rather than vanishing.
        assert_eq!(group_of("something-else"), "army");
    }

    /// A state does not decay and a sample does, and drawing them the same way says one of them
    /// is lying.
    #[test]
    fn only_a_sampled_reading_has_an_age() {
        let sampled =
            Diagnostic::new("system.cpu", Health::Healthy, "load 2.1", Kind::Sampled).measured(100);
        assert_eq!(age_secs(&sampled, 160), Some(60));
        assert!(stale(&sampled, 200));
        assert!(!stale(&sampled, 110));

        let state = Diagnostic::new(
            "army.tasks",
            Health::Healthy,
            "1 in hand",
            Kind::EventDriven,
        );
        assert_eq!(age_secs(&state, 9_999), None, "a state does not age");
        assert!(!stale(&state, 9_999), "and can never be stale");
    }

    /// Nothing measured it, so there is no age and nothing to invent.
    #[test]
    fn a_sample_nobody_took_has_no_age_rather_than_a_zero() {
        let never = Diagnostic::new("system.gpu", Health::Unknown, "no card", Kind::Sampled);
        assert_eq!(never.measured_at, None);
        assert_eq!(age_secs(&never, 500), None);
        assert!(!stale(&never, 500), "unmeasured is absent, not stale");
    }
}
