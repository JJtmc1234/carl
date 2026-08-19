//! Health, split by where the number came from.
//!
//! Two boards, army and system, because they answer different questions and mixing them makes
//! both harder to scan. Inside each, the worst sorts to the top, so the screen orders itself
//! by what needs somebody.
//!
//! The freshness rule is the one that keeps this honest. A sampled figure carries the age of
//! the reading and greys out once it is too old to present as current. An event driven state
//! carries no age at all, because it is true until something changes it and putting a clock
//! beside it would suggest otherwise. A component nothing has measured shows a gap, never a
//! zero.

use eframe::egui::{Align, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::command::WorkspaceRequest;
use crate::model::{Diagnostic, Health, board_of, stale};
use crate::theme;

use super::{shell, widgets};

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.5);
    ui.horizontal_top(|ui| {
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| board(app, ui, "army", "ARMY"));
        });
        ui.add_space(16.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| board(app, ui, "system", "SYSTEM"));
        });
    });
}

fn board(app: &mut App, ui: &mut Ui, group: &str, title: &str) {
    widgets::section(ui, title);
    let now = app.snapshot.at;
    // Cloned out first so the closure below does not hold a borrow of the snapshot while it
    // also needs to call back into the app.
    let rows: Vec<Diagnostic> = sorted(&app.snapshot.diagnostics, group)
        .into_iter()
        .cloned()
        .collect();
    let mut investigate: Option<String> = None;

    ScrollArea::vertical()
        .id_salt(format!("diag-{group}"))
        .show(ui, |ui| {
            for d in &rows {
                let is_stale = stale(d, now);
                let color = widgets::health_color(d.health);
                let response = widgets::row(ui, 44.0, false, false, |ui| {
                    ui.horizontal(|ui| {
                        widgets::pip(ui, color, d.health != Health::Unknown);
                        ui.label(
                            RichText::new(&d.component)
                                .font(theme::body())
                                .color(theme::TEXT),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(widgets::health_label(d.health))
                                    .font(theme::label())
                                    .color(color),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        // A stale sample is dimmed rather than hidden, so it is visibly old
                        // instead of quietly passing as current.
                        let summary_color = if is_stale { theme::UNKNOWN } else { theme::DIM };
                        ui.label(
                            RichText::new(&d.summary)
                                .font(theme::label())
                                .color(summary_color),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(f) = widgets::freshness(d, now) {
                                ui.label(
                                    RichText::new(f).font(theme::label()).color(if is_stale {
                                        theme::WARN
                                    } else {
                                        theme::FAINT
                                    }),
                                );
                            }
                        });
                    });
                    // `metric_pairs` renders an unreadable value as the word unknown rather
                    // than as an empty string, so a gap can never be mistaken for a zero.
                    let pairs = d.metric_pairs();
                    if !pairs.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (k, v) in &pairs {
                                ui.label(
                                    RichText::new(format!("{k} {v}"))
                                        .font(theme::label())
                                        .color(theme::FAINT),
                                );
                            }
                        });
                    }
                });
                if response.clicked() {
                    investigate = Some(d.component.clone());
                }
            }
        });

    if let Some(component) = investigate {
        app.open_workspace(WorkspaceRequest::Investigate { component });
    }
}

/// One board's rows, worst first and then by name so the order is stable frame to frame.
pub fn sorted<'a>(all: &'a [Diagnostic], group: &str) -> Vec<&'a Diagnostic> {
    let mut rows: Vec<&Diagnostic> = all.iter().filter(|d| board_of(d) == group).collect();
    // Worst first, then by name so the order is stable frame to frame and rows do not swap
    // places under the pointer.
    rows.sort_by(|a, b| {
        worst_first(a.health)
            .cmp(&worst_first(b.health))
            .then(a.component.cmp(&b.component))
    });
    rows
}

/// The order a board is read in. Worst first, and unknown above healthy because a gap is worth
/// noticing before something that is fine.
fn worst_first(h: Health) -> u8 {
    match h {
        Health::Failed => 0,
        Health::Blocked => 1,
        Health::Degraded => 2,
        Health::Unknown => 3,
        Health::Healthy => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Kind;

    fn d(component: &str, health: Health) -> Diagnostic {
        Diagnostic::new(component, health, "", Kind::EventDriven)
    }

    /// The screen must order itself by what needs somebody, and stably, or rows swap places
    /// under the pointer every frame.
    #[test]
    fn the_worst_sorts_to_the_top_and_ties_are_stable() {
        let all = vec![
            d("army.b-healthy", Health::Healthy),
            d("army.a-unknown", Health::Unknown),
            d("army.c-failed", Health::Failed),
            d("army.a-degraded", Health::Degraded),
            d("army.b-degraded", Health::Degraded),
        ];
        let order: Vec<&str> = sorted(&all, "army")
            .iter()
            .map(|d| d.component.as_str())
            .collect();
        assert_eq!(
            order,
            vec![
                "army.c-failed",
                "army.a-degraded",
                "army.b-degraded",
                "army.a-unknown",
                "army.b-healthy"
            ]
        );
    }

    /// The split is by prefix, and it has to hold for the ids on main today as well as the
    /// ones Process 3 is renaming to.
    #[test]
    fn a_board_shows_only_its_own_group() {
        let all = vec![
            d("army.carl", Health::Healthy),
            d("system.cpu", Health::Healthy),
            d("agent.nora", Health::Healthy),
            d("system.disk:/", Health::Healthy),
        ];
        assert_eq!(
            sorted(&all, "army").len(),
            2,
            "army and the legacy agent id"
        );
        assert_eq!(sorted(&all, "system").len(), 2);
    }

    /// An unreadable metric renders as the word unknown, never as a zero somebody could act on.
    #[test]
    fn an_unreadable_metric_never_renders_as_a_number() {
        use carl::providers::health::{Metric, Reading};
        let gpu = Diagnostic::new("system.gpu", Health::Unknown, "no card", Kind::Sampled)
            .with(Metric::new("vram", Reading::Unknown, "MiB"));

        let pairs = gpu.metric_pairs();
        assert_eq!(pairs.len(), 1);
        assert!(
            pairs[0].1.to_lowercase().contains("unknown"),
            "got {:?}, which a reader could mistake for a measurement",
            pairs[0].1
        );
        assert!(!pairs[0].1.contains('0'), "and it must not read as zero");
    }
}
