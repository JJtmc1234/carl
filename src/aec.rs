//! Finding the echo cancelled microphone and speaker, when they are running.
//!
//! `etc/carl-aec.conf` creates a pair of virtual devices: a sink Carl plays into and a source
//! that is the real microphone with whatever went into that sink subtracted out. Measured on
//! this machine, Carl's own voice arrives at the plain microphone at rms 0.048 and at the
//! cancelled one at rms 0.0017. That is twenty eight times quieter, which puts it under the
//! noise floor of a quiet room.
//!
//! It has to be optional. The canceller is a separate process and it can be stopped, and Carl
//! refusing to run because a nicety is missing would be worse than Carl running half duplex.
//! So this reports what it found and the caller decides.

use std::process::Command;

/// The sink Carl plays into. Named in `etc/carl-aec.conf`.
pub const SINK: &str = "carl-speaker";
/// The source Carl listens to, with his own voice already removed.
pub const SOURCE: &str = "carl-mic";

/// Which devices Carl should use, and whether he can hear while speaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Devices {
    /// The canceller is running. Carl can listen and talk at once, so he can be interrupted.
    Cancelled,
    /// No canceller. Carl must mute the microphone whenever he speaks, or he hears himself.
    Plain,
}

impl Devices {
    /// Asks the audio server what exists right now.
    ///
    /// Exactly one of each is required, and both the missing case and the duplicated case are
    /// refused.
    ///
    /// Missing is obvious. Duplicated is not, and it is worse. Two cancellers create two sinks
    /// called `carl-speaker` and two sources called `carl-mic`, and naming one picks whichever
    /// the server feels like. Carl then plays into one canceller and records from the other,
    /// so the one listening has no idea what the one playing is doing and subtracts nothing.
    ///
    /// That happened. A canceller was started by hand and then again as a service, and the
    /// result was Carl answering himself in a loop, each reply becoming the next question,
    /// while every check said the audio was echo cancelled. Falling back to muting is worse
    /// audio and obviously correct behaviour, which beats better audio that silently is not.
    pub fn detect() -> Self {
        let Ok(out) = Command::new("pw-dump").output() else {
            return Self::Plain;
        };
        if !out.status.success() {
            return Self::Plain;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Self::from_counts(count(&text, SINK), count(&text, SOURCE))
    }

    /// The rule, separated from asking the audio server so it can be tested without one.
    pub fn from_counts(sinks: usize, sources: usize) -> Self {
        match (sinks, sources) {
            (1, 1) => Self::Cancelled,
            (0, _) | (_, 0) => Self::Plain,
            _ => {
                eprintln!(
                    "there are {sinks} sinks called {SINK} and {sources} sources called \
                     {SOURCE}, so naming one picks an unpredictable half of an unpredictable \
                     pair. Carl will mute the microphone while speaking instead. Stop the \
                     spare canceller: pgrep -af carl-aec.conf"
                );
                Self::Plain
            }
        }
    }

    /// The device name to hand `arecord`, and the source to select through the environment.
    pub fn source(&self) -> Option<&'static str> {
        match self {
            Self::Cancelled => Some(SOURCE),
            Self::Plain => None,
        }
    }

    pub fn sink(&self) -> Option<&'static str> {
        match self {
            Self::Cancelled => Some(SINK),
            Self::Plain => None,
        }
    }

    /// Whether Carl can be interrupted mid sentence.
    pub fn can_barge_in(&self) -> bool {
        matches!(self, Self::Cancelled)
    }
}

/// How many nodes carry this exact name.
fn count(dump: &str, name: &str) -> usize {
    dump.matches(&format!("\"node.name\": \"{name}\"")).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half a canceller is worse than none, because Carl would believe he cannot hear
    /// himself while hearing himself at full volume.
    #[test]
    fn plain_devices_offer_nothing_and_promise_nothing() {
        let p = Devices::Plain;
        assert_eq!(p.source(), None);
        assert_eq!(p.sink(), None);
        assert!(!p.can_barge_in());
    }

    /// One of each is the only shape that works.
    #[test]
    fn exactly_one_pair_is_the_cancelled_case() {
        assert_eq!(Devices::from_counts(1, 1), Devices::Cancelled);
    }

    /// The failure that made Carl answer himself in a loop while every check said the audio
    /// was echo cancelled. Two cancellers, so naming a device picks an unpredictable half of
    /// an unpredictable pair, and the one listening does not know what the one playing is up
    /// to. Muting is worse audio and obviously right, which beats better audio that is not.
    #[test]
    fn two_cancellers_are_refused_rather_than_gambled_on() {
        assert_eq!(Devices::from_counts(2, 2), Devices::Plain);
        assert_eq!(Devices::from_counts(2, 1), Devices::Plain);
        assert_eq!(Devices::from_counts(1, 2), Devices::Plain);
    }

    /// Half a canceller is worse than none, because Carl would believe he could not hear
    /// himself while hearing himself at full volume.
    #[test]
    fn a_missing_half_is_refused() {
        assert_eq!(Devices::from_counts(0, 1), Devices::Plain);
        assert_eq!(Devices::from_counts(1, 0), Devices::Plain);
        assert_eq!(Devices::from_counts(0, 0), Devices::Plain);
    }

    #[test]
    fn nodes_are_counted_by_their_exact_name() {
        let dump =
            r#"{"node.name": "carl-mic"} {"node.name": "carl-speaker"} {"node.name": "carl-mic"}"#;
        assert_eq!(count(dump, SOURCE), 2);
        assert_eq!(count(dump, SINK), 1);
        assert_eq!(count(dump, "carl-nothing"), 0);
    }

    #[test]
    fn cancelled_devices_come_as_a_pair() {
        let c = Devices::Cancelled;
        assert_eq!(c.source(), Some(SOURCE));
        assert_eq!(c.sink(), Some(SINK));
        assert!(c.can_barge_in());
    }
}
