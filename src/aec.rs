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
    /// Both halves are required. One without the other means a half configured canceller,
    /// and using the cancelled source while playing to the ordinary speakers is worse than
    /// using neither: the canceller would be subtracting silence and Carl would hear himself
    /// at full volume while believing he could not.
    pub fn detect() -> Self {
        match Command::new("pw-dump").output() {
            Ok(out) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                let has = |name: &str| text.contains(&format!("\"node.name\": \"{name}\""));
                if has(SINK) && has(SOURCE) {
                    Self::Cancelled
                } else {
                    Self::Plain
                }
            }
            _ => Self::Plain,
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

    #[test]
    fn cancelled_devices_come_as_a_pair() {
        let c = Devices::Cancelled;
        assert_eq!(c.source(), Some(SOURCE));
        assert_eq!(c.sink(), Some(SINK));
        assert!(c.can_barge_in());
    }
}
