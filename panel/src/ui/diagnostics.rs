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
use crate::model::{Diagnostic, Health};
use crate::theme;

use super::{shell, widgets};

/// How old a sampled reading may be before it stops being shown as current.
pub const STALE_AFTER: u64 = 30;

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
                let stale = d.stale(now, STALE_AFTER);
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
                                RichText::new(d.health.label())
                                    .font(theme::label())
                                    .color(color),
                            );
                        });
                    });
                    ui.horizontal(|ui| {
                        // A stale sample is dimmed rather than hidden, so it is visibly old
                        // instead of quietly passing as current.
                        let summary_color = if stale { theme::UNKNOWN } else { theme::DIM };
                        ui.label(
                            RichText::new(&d.summary)
                                .font(theme::label())
                                .color(summary_color),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if let Some(f) = widgets::freshness(d.reading, d.measured_at, now) {
                                ui.label(RichText::new(f).font(theme::label()).color(if stale {
                                    theme::WARN
                                } else {
                                    theme::FAINT
                                }));
                            }
                        });
                    });
                    if !d.metrics.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (k, v) in &d.metrics {
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
    let mut rows: Vec<&Diagnostic> = all.iter().filter(|d| d.group == group).collect();
    rows.sort_by(|a, b| a.health.cmp(&b.health).then(a.component.cmp(&b.component)));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Reading;

    fn d(component: &str, health: Health) -> Diagnostic {
        Diagnostic {
            component: component.into(),
            group: "army".into(),
            health,
            summary: String::new(),
            metrics: Vec::new(),
            reading: Reading::EventDriven,
            measured_at: Some(0),
        }
    }

    /// The screen must order itself by what needs somebody, and stably, or rows swap places
    /// under the pointer every frame.
    #[test]
    fn the_worst_sorts_to_the_top_and_ties_are_stable() {
        let all = vec![
            d("b.healthy", Health::Healthy),
            d("a.unknown", Health::Unknown),
            d("c.failed", Health::Failed),
            d("a.degraded", Health::Degraded),
            d("b.degraded", Health::Degraded),
        ];
        let order: Vec<&str> = sorted(&all, "army")
            .iter()
            .map(|d| d.component.as_str())
            .collect();
        assert_eq!(
            order,
            vec![
                "c.failed",
                "a.degraded",
                "b.degraded",
                "a.unknown",
                "b.healthy"
            ]
        );
    }

    #[test]
    fn a_board_shows_only_its_own_group() {
        let mut all = vec![d("army.carl", Health::Healthy)];
        let mut sys = d("system.cpu", Health::Healthy);
        sys.group = "system".into();
        all.push(sys);

        assert_eq!(sorted(&all, "army").len(), 1);
        assert_eq!(sorted(&all, "system").len(), 1);
    }
}
