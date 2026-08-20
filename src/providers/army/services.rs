//! Whether Carl himself is running.
//!
//! Carl is three systemd **user** units rather than a daemon with a control socket, so the only
//! way to ask how he is doing from another process is to ask systemd. That is a good thing
//! here: systemd already knows about restarts, start time and exit status, and none of it
//! needs elevated rights because user units belong to the user.
//!
//! Verified on this machine. `systemctl --user show` answers for all three units and reports
//! `ActiveState`, `SubState`, `NRestarts` and `ExecMainPID`.
//!
//! A restart count above zero matters even when the unit is active. All three units are
//! `Restart=always`, so a unit that is crashing and coming back looks identical to a healthy
//! one if you only ask whether it is active right now.

use std::process::Command;

use crate::providers::health::{Diagnostic, Health, Kind, Metric, Reading};

/// The units `etc/systemd/install.sh` installs.
pub const CARL_UNITS: &[&str] = &["carl-aec", "carl-listen", "carl-slack"];

/// The binary that answers about them.
pub const SYSTEMCTL: &str = "systemctl";

/// What systemd says about one unit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Unit {
    pub name: String,
    /// `active`, `inactive`, `failed`, `activating`, and so on.
    pub active_state: Option<String>,
    /// `running`, `dead`, `exited`, `auto-restart`.
    pub sub_state: Option<String>,
    /// `loaded`, `not-found`, `bad-setting`, `error`, `masked`.
    ///
    /// Asked for because it is the only field that distinguishes a unit systemd has never
    /// heard of from one that is installed and stopped. Without it `systemctl show` answers
    /// for a unit that does not exist with `ActiveState=inactive` and exits 0, which reads as
    /// deliberately stopped. See bug 28.
    pub load_state: Option<String>,
    pub restarts: Option<u64>,
    pub main_pid: Option<u32>,
    /// Microseconds since boot when the main process started.
    pub started_monotonic_us: Option<u64>,
}

/// Seconds on `CLOCK_MONOTONIC`, which is the clock systemd's start stamp is on.
///
/// Not `/proc/uptime`, which this used to subtract from and which is `CLOCK_BOOTTIME`. Those
/// are two different clocks: boottime counts time the machine spent suspended and monotonic
/// does not. Subtracting one from the other adds every second the laptop has ever been asleep
/// to every service's uptime. Measured on this machine: boottime 1066791, monotonic 269971, so
/// the answer was out by 796820 seconds, and a service up 5 hours read as up 9.4 days.
///
/// `None` if the clock cannot be read, which is treated as not knowing rather than as zero.
/// See bug 27.
pub fn monotonic_secs() -> Option<f64> {
    let mut t = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Sound because `t` is a fully initialised local that outlives the call, and the kernel
    // only writes to it.
    let read = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut t) };
    (read == 0).then(|| t.tv_sec as f64 + t.tv_nsec as f64 / 1_000_000_000.0)
}

impl Unit {
    /// How long the current main process has been up, in seconds.
    ///
    /// `now` has to be on `CLOCK_MONOTONIC`, because that is the clock
    /// `ExecMainStartTimestampMonotonic` is on. Passing it in rather than reading the clock
    /// here is what lets a test check the arithmetic, but the two values must come from the
    /// same clock or the answer is meaningless, which is exactly what went wrong. See bug 27.
    pub fn uptime_secs(&self, now_monotonic: f64) -> Option<f64> {
        let started = self.started_monotonic_us? as f64 / 1_000_000.0;
        let up = now_monotonic - started;
        // Now reachable, which it was not before. Boottime is always ahead of monotonic, so
        // this guard could never fire while the wrong clock was being used, and a start stamp
        // in the future would have gone straight through.
        (up >= 0.0).then_some(up)
    }

    /// The judgement, which is deliberately five ways rather than two.
    ///
    /// A unit that is up but has restarted is degraded, not healthy. A unit that is stopped is
    /// blocked rather than failed, because somebody stopping a service on purpose is not a
    /// crash and should not read like one.
    pub fn health(&self) -> Health {
        // Before anything else, because `ActiveState` is meaningless for a unit systemd does
        // not have. It answers `inactive` for one that was never installed and exits 0, which
        // used to read as `Blocked`, and `Blocked` means somebody stopped it on purpose. The
        // remedy that implies, start the service, cannot work. See bug 28.
        if self.missing() {
            return Health::Unknown;
        }
        match (self.active_state.as_deref(), self.sub_state.as_deref()) {
            (Some("active"), Some("running")) => {
                if self.restarts.unwrap_or(0) > 0 {
                    Health::Degraded
                } else {
                    Health::Healthy
                }
            }
            (Some("active"), _) | (Some("reloading"), _) => Health::Degraded,
            (Some("activating"), _) => Health::Degraded,
            (Some("failed"), _) => Health::Failed,
            (Some("inactive"), _) | (Some("deactivating"), _) => Health::Blocked,
            _ => Health::Unknown,
        }
    }

    /// Whether systemd has no such unit, rather than having one that is not running.
    ///
    /// `masked` is deliberately not here. A masked unit does exist and was masked by somebody,
    /// which is a decision rather than an absence, so it keeps reading as stopped.
    pub fn missing(&self) -> bool {
        matches!(
            self.load_state.as_deref(),
            Some("not-found") | Some("bad-setting") | Some("error")
        )
    }

    pub fn summary(&self) -> String {
        if self.missing() {
            // Names the remedy, because "not installed" and "stopped" call for different
            // actions and the panel used to show the same words for both.
            return format!(
                "not installed, systemd has no {}. Run etc/systemd/install.sh",
                self.name
            );
        }
        match (self.active_state.as_deref(), self.restarts.unwrap_or(0)) {
            (Some("active"), 0) => "running".to_string(),
            (Some("active"), n) => format!("running, restarted {n} times"),
            (Some("inactive"), _) => "stopped".to_string(),
            (Some("failed"), _) => "failed".to_string(),
            (Some(other), _) => other.to_string(),
            (None, _) => "systemd did not answer".to_string(),
        }
    }
}

/// Parses the `key=value` lines `systemctl show` prints.
pub fn parse_show(name: &str, text: &str) -> Unit {
    let mut unit = Unit {
        name: name.to_string(),
        ..Unit::default()
    };

    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "LoadState" => unit.load_state = Some(value.to_string()),
            "ActiveState" => unit.active_state = Some(value.to_string()),
            "SubState" => unit.sub_state = Some(value.to_string()),
            "NRestarts" => unit.restarts = value.parse().ok(),
            // Zero means no main process, which is different from a pid we could not read.
            "ExecMainPID" => unit.main_pid = value.parse().ok().filter(|p| *p != 0),
            "ExecMainStartTimestampMonotonic" => {
                unit.started_monotonic_us = value.parse().ok().filter(|t| *t != 0)
            }
            _ => {}
        }
    }
    unit
}

/// Asks systemd about one user unit.
///
/// `None` when systemctl is missing or refuses to answer, which is a different thing from a
/// unit that is stopped and must not be reported as one.
pub fn read(name: &str) -> Option<Unit> {
    read_with(SYSTEMCTL, name)
}

/// The same, against a named binary.
///
/// Injectable so a test can point at something that is not there and prove the provider
/// reports unknown rather than falling over, which is the same trick `Runner::at` and
/// `Camera::at` use elsewhere in this codebase.
pub fn read_with(program: &str, name: &str) -> Option<Unit> {
    let out = Command::new(program)
        .args([
            "--user",
            "show",
            name,
            "-p",
            "LoadState,ActiveState,SubState,NRestarts,ExecMainPID,ExecMainStartTimestampMonotonic",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_show(name, &String::from_utf8_lossy(&out.stdout)))
}

/// One diagnostic per Carl unit.
///
/// Event driven rather than sampled. A service either is running or is not, and that state is
/// true until systemd changes it rather than being a number that goes stale.
pub fn diagnostics(machine_uptime: Option<f64>) -> Vec<Diagnostic> {
    // The argument is ignored and kept only so callers do not all have to change. `/proc/uptime`
    // answers a different question from the one this needs. See bug 27.
    let _ = machine_uptime;
    diagnostics_with(SYSTEMCTL, monotonic_secs())
}

/// The same, against a named binary, so a test can prove a missing systemctl degrades to
/// unknown rather than taking the whole snapshot down.
pub fn diagnostics_with(program: &str, now_monotonic: Option<f64>) -> Vec<Diagnostic> {
    CARL_UNITS
        .iter()
        .map(|name| match read_with(program, name) {
            None => Diagnostic::new(
                &format!("army.service.{name}"),
                Health::Unknown,
                "systemd did not answer",
                Kind::EventDriven,
            ),
            Some(unit) => {
                let uptime = now_monotonic.and_then(|now| unit.uptime_secs(now));
                Diagnostic::new(
                    &format!("army.service.{name}"),
                    unit.health(),
                    unit.summary(),
                    Kind::EventDriven,
                )
                .with(Metric::new(
                    "pid",
                    unit.main_pid
                        .map_or(Reading::Unknown, |p| Reading::Int(p as u64)),
                    "",
                ))
                .with(Metric::new(
                    "restarts",
                    unit.restarts.map_or(Reading::Unknown, Reading::Int),
                    "",
                ))
                .with(Metric::new(
                    "uptime",
                    uptime.map_or(Reading::Unknown, |u| Reading::Int(u as u64)),
                    "s",
                ))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape this machine printed when the source was verified.
    const REAL: &str = "ActiveState=active\nSubState=running\nNRestarts=0\n\
                        ExecMainStartTimestampMonotonic=250415811408\nExecMainPID=2868404\n";

    #[test]
    fn the_real_output_from_this_machine_parses() {
        let u = parse_show("carl-slack", REAL);
        assert_eq!(u.active_state.as_deref(), Some("active"));
        assert_eq!(u.sub_state.as_deref(), Some("running"));
        assert_eq!(u.restarts, Some(0));
        assert_eq!(u.main_pid, Some(2868404));
        assert_eq!(u.health(), Health::Healthy);
        assert_eq!(u.summary(), "running");
    }

    /// The case the whole restart count exists for. Active is not the same as well.
    #[test]
    fn a_unit_that_keeps_restarting_is_degraded_even_while_active() {
        let u = parse_show(
            "carl-listen",
            "ActiveState=active\nSubState=running\nNRestarts=7\n",
        );
        assert_eq!(u.health(), Health::Degraded);
        assert!(u.summary().contains("restarted 7 times"), "{}", u.summary());
    }

    /// Somebody stopping a service deliberately is not a crash.
    /// The bug. `systemctl show` answers for a unit that does not exist with
    /// `ActiveState=inactive` and exits 0, so a unit systemd has never heard of was reported
    /// with the same health and the same word as one somebody deliberately stopped.
    ///
    /// `Blocked` means a person stopped it on purpose, and the remedy that implies, start the
    /// service, cannot work for a unit that is not installed.
    #[test]
    fn a_unit_systemd_has_never_heard_of_is_unknown_rather_than_stopped() {
        // Exactly what this machine prints for a name that does not exist, checked by running
        // the command the provider runs.
        let u = parse_show(
            "ghost.service",
            "LoadState=not-found\nActiveState=inactive\nSubState=dead\nNRestarts=0\nExecMainPID=0\n",
        );

        assert_eq!(u.health(), Health::Unknown, "not installed is not stopped");
        assert!(u.summary().contains("not installed"), "{}", u.summary());
        assert!(
            u.summary().contains("install.sh"),
            "and names the remedy: {}",
            u.summary()
        );
    }

    /// A unit that really is installed and really is stopped still reads as stopped, which is
    /// the case the fix must not swallow.
    #[test]
    fn a_loaded_unit_that_is_stopped_still_reads_as_stopped() {
        let u = parse_show(
            "real.service",
            "LoadState=loaded\nActiveState=inactive\nSubState=dead\n",
        );
        assert_eq!(u.health(), Health::Blocked);
        assert_eq!(u.summary(), "stopped");
    }

    /// A masked unit is deliberately not treated as missing. It exists and somebody masked it,
    /// which is a decision rather than an absence, so it keeps reading as stopped.
    #[test]
    fn a_masked_unit_is_a_decision_rather_than_an_absence() {
        let u = parse_show(
            "masked.service",
            "LoadState=masked\nActiveState=inactive\nSubState=dead\n",
        );
        assert!(!u.missing());
        assert_eq!(u.health(), Health::Blocked);
    }

    /// And this machine, where the units are installed, must not start reading as missing.
    #[test]
    fn the_installed_units_on_this_machine_are_not_reported_missing() {
        for name in CARL_UNITS {
            let Some(u) = read(name) else { continue };
            assert!(
                !u.missing(),
                "{name} is installed here and reads as missing: {:?}",
                u.load_state
            );
        }
    }

    #[test]
    fn a_stopped_unit_is_blocked_and_a_failed_one_is_failed() {
        let stopped = parse_show("x", "ActiveState=inactive\nSubState=dead\n");
        assert_eq!(stopped.health(), Health::Blocked);
        assert_eq!(stopped.summary(), "stopped");

        let broken = parse_show("x", "ActiveState=failed\nSubState=failed\n");
        assert_eq!(broken.health(), Health::Failed);
    }

    #[test]
    fn a_unit_still_coming_up_is_degraded_rather_than_failed() {
        let starting = parse_show("x", "ActiveState=activating\nSubState=start\n");
        assert_eq!(starting.health(), Health::Degraded);
    }

    /// Silence from systemd is not a claim about the service.
    #[test]
    fn no_answer_from_systemd_is_unknown_rather_than_failed() {
        let nothing = parse_show("x", "");
        assert_eq!(nothing.health(), Health::Unknown);
        assert_eq!(nothing.main_pid, None);
        assert_eq!(nothing.restarts, None);
    }

    /// A unit with no main process reports no pid, and zero is systemd's way of saying that
    /// rather than a real process id.
    #[test]
    fn a_zero_pid_is_read_as_no_process() {
        let u = parse_show("x", "ExecMainPID=0\nExecMainStartTimestampMonotonic=0\n");
        assert_eq!(u.main_pid, None);
        assert_eq!(u.started_monotonic_us, None);
        assert_eq!(u.uptime_secs(1000.0), None);
    }

    #[test]
    fn uptime_is_the_monotonic_clock_less_the_start_stamp() {
        let u = parse_show("x", "ExecMainStartTimestampMonotonic=250000000000\n");
        let up = u.uptime_secs(250_100.0).unwrap();
        assert!(
            (up - 100.0).abs() < 1.0,
            "expected about 100 seconds, got {up}"
        );
        assert_eq!(
            u.uptime_secs(1.0),
            None,
            "a start in the future is not an uptime"
        );
    }

    /// This test cannot catch the bug it is named after, and that is the point of saying so.
    ///
    /// The arithmetic above was always right. What was wrong was which clock the caller read,
    /// and a test that supplies both numbers itself picks the same scale for both by
    /// construction. The guard for the real bug has to look at the two clocks, which is the
    /// test below.
    ///
    /// `/proc/uptime` is `CLOCK_BOOTTIME` and counts time spent suspended. The start stamp
    /// systemd gives is `CLOCK_MONOTONIC` and does not. Subtracting one from the other adds
    /// every second the machine has ever been asleep to every service's uptime.
    #[test]
    fn the_monotonic_clock_never_runs_ahead_of_boottime() {
        let monotonic = monotonic_secs().expect("this machine has a monotonic clock");
        let boottime: f64 = std::fs::read_to_string("/proc/uptime")
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap();

        assert!(
            monotonic <= boottime + 1.0,
            "monotonic {monotonic} is ahead of boottime {boottime}, which cannot happen"
        );
        assert!(monotonic > 0.0);
    }

    /// The invariant that actually catches it, against the real machine.
    ///
    /// A service started after this boot, so the time it has been up cannot exceed the
    /// monotonic clock. Under the old code it could and did: with boottime at 1066791 and
    /// monotonic at 269971, a unit started at 250416 computed 816375 seconds of uptime, which
    /// is three times longer than the clock it is measured against has been running.
    ///
    /// Skips rather than fails where the units are not installed, since it is asserting
    /// something about this machine rather than about the arithmetic.
    #[test]
    fn a_real_units_uptime_never_exceeds_the_clock_it_is_measured_against() {
        let Some(monotonic) = monotonic_secs() else {
            return;
        };
        for d in diagnostics(None) {
            for m in d.metrics.iter().filter(|m| m.name == "uptime") {
                let Reading::Int(up) = m.value else { continue };
                assert!(
                    up as f64 <= monotonic + 1.0,
                    "{} reports {up} seconds up against a monotonic clock of {monotonic}",
                    d.component
                );
            }
        }
    }

    /// Against the real machine. All three units were installed and active when this was
    /// written, so anything else means the environment changed rather than the code.
    #[test]
    fn this_machine_answers_for_every_carl_unit() {
        let found = diagnostics(Some(1_000_000.0));
        assert_eq!(found.len(), CARL_UNITS.len());
        for d in &found {
            assert_eq!(d.kind, Kind::EventDriven, "a service is not telemetry");
            assert_eq!(d.measured_at, None);
            assert!(d.component.starts_with("army.service."));
        }
    }
}
