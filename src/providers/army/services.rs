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
    pub restarts: Option<u64>,
    pub main_pid: Option<u32>,
    /// Microseconds since boot when the main process started.
    pub started_monotonic_us: Option<u64>,
}

impl Unit {
    /// How long the current main process has been up, given the machine's uptime in seconds.
    pub fn uptime_secs(&self, machine_uptime: f64) -> Option<f64> {
        let started = self.started_monotonic_us? as f64 / 1_000_000.0;
        let up = machine_uptime - started;
        (up >= 0.0).then_some(up)
    }

    /// The judgement, which is deliberately five ways rather than two.
    ///
    /// A unit that is up but has restarted is degraded, not healthy. A unit that is stopped is
    /// blocked rather than failed, because somebody stopping a service on purpose is not a
    /// crash and should not read like one.
    pub fn health(&self) -> Health {
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

    pub fn summary(&self) -> String {
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
            "ActiveState,SubState,NRestarts,ExecMainPID,ExecMainStartTimestampMonotonic",
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
    diagnostics_with(SYSTEMCTL, machine_uptime)
}

/// The same, against a named binary, so a test can prove a missing systemctl degrades to
/// unknown rather than taking the whole snapshot down.
pub fn diagnostics_with(program: &str, machine_uptime: Option<f64>) -> Vec<Diagnostic> {
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
                let uptime = machine_uptime.and_then(|m| unit.uptime_secs(m));
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
    fn uptime_is_machine_uptime_less_the_start_stamp() {
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
