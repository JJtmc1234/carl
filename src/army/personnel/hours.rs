//! When an agent is meant to be off.
//!
//! An agent that never stops is an agent whose context and cost grow without anybody choosing
//! it. That is the whole argument for a sleep window, and it is worth keeping in view, because
//! the feature reads like a nicety and is actually the only thing standing between a long
//! weekend and a bill nobody decided to pay.
//!
//! **Hours, not minutes.** A window of 23 to 7 is a decision somebody makes once and reads back
//! a year later. Minutes would let it be 23:47, which nobody wants and which makes every test
//! and every log line longer for it.
//!
//! **Local time, because a person set it.** Somebody writing "asleep from eleven at night" means
//! eleven where they are, and a window stored in UTC would be right for one part of the year in
//! one country. The conversion happens once, in `local_hour`, and everything else here takes an
//! hour of the day and is therefore ordinary arithmetic that a test can cover completely.
//!
//! **This is not authority.** It says when a process should exist, and nothing about what the
//! agent may do while it does. An agent editing its own window would be an agent choosing to run
//! all night, which costs money, so it is worth saying that the file this lives in is the same
//! `config.json` that already holds the model, and the same argument applies to both.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// The window an agent is asleep for, in local hours.
///
/// `from` is the first hour it is off and `to` is the first hour it is back, so 23 to 7 means
/// asleep at 23:00 and running again at 07:00. Half open at both ends, which is the only
/// convention where two windows can be adjacent without overlapping or leaving a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hours {
    pub from: u8,
    pub to: u8,
}

impl Hours {
    pub fn new(from: u8, to: u8) -> Result<Self> {
        let hours = Self { from, to };
        hours.check()?;
        Ok(hours)
    }

    /// The ordinary overnight window: off at eleven, back at seven.
    ///
    /// A default rather than a decision. Which hours an army keeps is JJ's to change, one
    /// `config.json` at a time, and this is what it is until he does.
    pub fn night() -> Self {
        Self { from: 23, to: 7 }
    }

    /// Whether an agent with this window should be off at the given local hour.
    ///
    /// The wrap is the whole function. An overnight window runs past midnight, so `from` is
    /// greater than `to` and the test is an or rather than an and. Getting that backwards gives
    /// a window that is asleep for sixteen hours and awake all night, which is exactly wrong and
    /// looks almost right.
    pub fn asleep_at(&self, hour: u32) -> bool {
        let hour = hour as u8;
        match self.from < self.to {
            true => hour >= self.from && hour < self.to,
            false => hour >= self.from || hour < self.to,
        }
    }

    /// How long the window is, in hours. Only for saying it out loud.
    pub fn length(&self) -> u8 {
        match self.from < self.to {
            true => self.to - self.from,
            false => 24 - self.from + self.to,
        }
    }

    /// A window that cannot be acted on is refused at load rather than at three in the morning.
    ///
    /// Equal ends are the interesting refusal. They could mean asleep for nothing or asleep for
    /// the whole day, both readings are defensible, and an agent that is off forever because
    /// somebody meant the other one is not a bug anybody finds quickly.
    pub fn check(&self) -> Result<()> {
        if self.from > 23 || self.to > 23 {
            return Err(Error::Refused(format!(
                "a sleep window is between 0 and 23 hours, got {} to {}",
                self.from, self.to
            )));
        }
        if self.from == self.to {
            return Err(Error::Refused(
                "a sleep window that starts and ends at the same hour could mean never or \
                 always, so it is refused rather than guessed at"
                    .into(),
            ));
        }
        Ok(())
    }
}

impl std::fmt::Display for Hours {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}:00 to {:02}:00", self.from, self.to)
    }
}

/// The hour of the day where this machine is, for a unix timestamp.
///
/// The one impure function here, and it is one call. `localtime_r` is what applies the zone and
/// whatever daylight saving is in force, which is a table this program has no business owning a
/// copy of.
pub fn local_hour(unix: u64) -> Option<u32> {
    let seconds = unix as libc::time_t;
    let mut broken: libc::tm = unsafe { std::mem::zeroed() };

    // Safe: `broken` is owned here and lives past the call, and the result is only read when the
    // pointer comes back non null, which is how localtime_r reports failure.
    let filled = unsafe { libc::localtime_r(&seconds, &mut broken) };
    match filled.is_null() {
        true => None,
        false => Some(broken.tm_hour as u32),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daytime_window_is_asleep_between_its_ends() {
        let hours = Hours::new(9, 17).unwrap();
        assert!(!hours.asleep_at(8));
        assert!(hours.asleep_at(9), "the first hour is inside");
        assert!(hours.asleep_at(16));
        assert!(!hours.asleep_at(17), "the last hour is not");
        assert_eq!(hours.length(), 8);
    }

    /// The case the whole type exists for, and the one where an and instead of an or gives a
    /// window that is asleep all day and awake all night while looking almost right.
    #[test]
    fn an_overnight_window_wraps_past_midnight() {
        let hours = Hours::night();
        assert!(hours.asleep_at(23));
        assert!(hours.asleep_at(0));
        assert!(hours.asleep_at(3));
        assert!(hours.asleep_at(6));
        assert!(!hours.asleep_at(7), "back at seven");
        assert!(!hours.asleep_at(12));
        assert!(!hours.asleep_at(22));
        assert_eq!(hours.length(), 8);
    }

    /// Every hour of the day belongs to exactly one side of the window, whichever way round it
    /// is. Anything else is a window with a gap or an overlap in it.
    #[test]
    fn every_hour_is_answered_for_either_direction() {
        for (from, to) in [(23, 7), (9, 17), (0, 1), (23, 22)] {
            let hours = Hours::new(from, to).unwrap();
            let asleep = (0..24).filter(|h| hours.asleep_at(*h)).count();
            assert_eq!(
                asleep,
                hours.length() as usize,
                "{hours} covers the wrong number of hours"
            );
        }
    }

    #[test]
    fn a_window_that_cannot_be_acted_on_is_refused() {
        assert!(Hours::new(24, 7).is_err(), "there is no hour 24");
        assert!(Hours::new(7, 24).is_err());
        assert!(
            Hours::new(7, 7).is_err(),
            "never or always, and nobody can tell which"
        );
        assert!(
            Hours::new(0, 23).is_ok(),
            "nearly all day is still a choice"
        );
    }

    #[test]
    fn a_window_round_trips_and_reads_as_a_time() {
        let before = Hours::night();
        let text = serde_json::to_string(&before).unwrap();
        assert_eq!(serde_json::from_str::<Hours>(&text).unwrap(), before);
        assert_eq!(before.to_string(), "23:00 to 07:00");
    }

    /// There is no fixed answer to check against without owning a copy of the zone table, which
    /// is the thing this deliberately does not do. What can be checked is that it answers, that
    /// the answer is an hour, and that it moves the way an hour does.
    #[test]
    fn the_local_hour_is_an_hour_and_advances_with_the_clock() {
        let noonish = 1_787_000_000;
        let hour = local_hour(noonish).expect("this machine has a clock");
        assert!(hour < 24, "{hour} is not an hour of the day");

        let later = local_hour(noonish + 3600).expect("still a clock");
        assert_eq!(later, (hour + 1) % 24, "an hour later is the next hour");
    }
}
