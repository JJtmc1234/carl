//! Diagnostics, narrowed for drawing without asking the collector to send less.
//!
//! Process 3 keeps a sampled unknown carrying its `measured_at` and its metric names, and they
//! were right to. "I looked at 14:32 and there is no NVIDIA card" and "I have never looked" are
//! different facts, and a collector that collapses them cannot be un collapsed downstream.
//!
//! So the panel keeps both and draws the difference, and the one thing it will not do is turn
//! an unknown into a zero. A number on this screen is a number somebody measured.

use carl::panel::view::{DiagnosticView, Metric};

use crate::model::{Diagnostic, Reading};

use super::health_of;

/// Which board a component belongs on, from its name.
///
/// Derived from the prefix rather than carried as a field, because the split into two boards is
/// this screen's idea and the collector should not have to know about it. The prefixes are
/// stable and agreed with Process 3.
pub fn group_of(component: &str) -> &'static str {
    match component.split('.').next() {
        Some("system") => "system",
        _ => "army",
    }
}

/// Whether a reading decays.
///
/// Machine numbers are true at the instant they were taken. Army state is true until something
/// changes it. Showing both with the same freshness cue would say one of them is lying.
pub fn reading_of(component: &str) -> Reading {
    match group_of(component) {
        "system" => Reading::Sampled,
        _ => Reading::EventDriven,
    }
}

/// One reading, in the shape the board draws.
pub fn one_diagnostic(wire: &DiagnosticView) -> Diagnostic {
    Diagnostic {
        component: wire.component.clone(),
        group: group_of(&wire.component).to_string(),
        health: health_of(wire.health),
        summary: wire.summary.clone(),
        metrics: wire.metrics.iter().map(metric).collect(),
        reading: reading_of(&wire.component),
        // Zero is not a time anybody measured at. The wire carries a plain u64 because most
        // components always have one, so the panel reads the sentinel back into the absence it
        // stands for rather than drawing 1 January 1970.
        measured_at: (wire.measured_at != 0).then_some(wire.measured_at),
    }
}

/// A metric, formatted once, here.
///
/// Formatted rather than passed through as a number so the drawing never does unit maths on
/// somebody else's figures. Guessing at a unit is how a percentage becomes a temperature.
fn metric(m: &Metric) -> (String, String) {
    let value = if m.value.fract() == 0.0 && m.value.abs() < 1e15 {
        format!("{}", m.value as i64)
    } else {
        format!("{:.1}", m.value)
    };
    match &m.unit {
        Some(unit) => (m.name.clone(), format!("{value} {unit}")),
        None => (m.name.clone(), value),
    }
}
