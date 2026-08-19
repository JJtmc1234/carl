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
/// Straight through to `Diagnostic::group()`, which is the contract now: every id is `army.`
/// or `system.` and anything else is a stray. The panel used to classify prefixes itself and
/// no longer does, so there is one answer to this question rather than two.
///
/// A stray lands on the army board rather than vanishing, because a component nobody can
/// place is still a component somebody should see.
pub fn board_of(d: &Diagnostic) -> &'static str {
    match d.group() {
        "system" => "system",
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

    /// The board comes from the canonical `group()`, and a stray still has to be visible
    /// somewhere rather than falling off the screen.
    #[test]
    fn the_board_comes_from_the_canonical_group() {
        let of = |c: &str| board_of(&Diagnostic::new(c, Health::Healthy, "", Kind::EventDriven));
        assert_eq!(of("system.cpu"), "system");
        assert_eq!(of("system.disk:/"), "system");
        assert_eq!(of("army.agent.nora"), "army");
        assert_eq!(of("army.service.carl-slack"), "army");

        // `group()` calls this unknown. It must still be drawn, not dropped.
        assert_eq!(of("something-else"), "army");
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
