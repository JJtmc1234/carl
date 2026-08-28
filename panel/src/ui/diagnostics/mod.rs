//! Health, as structure rather than as text.
//!
//! Two boards, army and system, because they answer different questions and mixing them makes
//! both harder to scan. Inside each, the worst sorts to the top, so the screen orders itself by
//! what needs somebody. Over each board sits a tally, so a board can be read as a whole before
//! any single row on it is.
//!
//! The old version was four lines of text per component with the metrics wrapped along the
//! bottom, and at any real density they collided. Every row is a card now, and the metrics are
//! laid out in fixed columns that are worked out from the card's width, which is what makes a
//! collision impossible rather than unlikely.
//!
//! The freshness rule is what keeps this honest. A sampled figure carries the age of the
//! reading and greys out once it is too old to present as current. An event driven state
//! carries no age at all, because it is true until something changes it. A component nothing
//! has measured shows a gap, never a zero.

use eframe::egui::{ScrollArea, Ui, vec2};

use crate::app::App;
use crate::command::WorkspaceRequest;
use crate::model::Diagnostic;
use crate::theme;

use super::{shell, widgets};

mod card;
pub mod order;

pub use order::sorted;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.5);
    let widths = [left, right];
    let mut investigate: Option<String> = None;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, (group, title)) in order::BOARDS.iter().enumerate() {
            if i > 0 {
                ui.add_space(theme::GAP + 4.0);
            }
            ui.allocate_ui(vec2(widths[i], ui.available_height()), |ui| {
                ui.vertical(|ui| {
                    if let Some(picked) = board(app, ui, group, title) {
                        investigate = Some(picked);
                    }
                });
            });
        }
    });

    if let Some(component) = investigate {
        app.open_workspace(WorkspaceRequest::Investigate { component });
    }
}

/// One board: a summary of the whole thing, then every row on it worst first.
fn board(app: &App, ui: &mut Ui, group: &str, title: &str) -> Option<String> {
    let now = app.snapshot.at;
    let rows: Vec<Diagnostic> = sorted(&app.snapshot.diagnostics, group)
        .into_iter()
        .cloned()
        .collect();

    summary(ui, &app.snapshot.diagnostics, group, title);
    ui.add_space(theme::GAP);

    if rows.is_empty() {
        widgets::card(ui, 92.0, widgets::Card::default(), |ui| {
            widgets::state_chip(
                ui,
                widgets::Mark::Dash,
                "NOTHING ON THIS BOARD",
                theme::UNKNOWN,
            );
            ui.add_space(6.0);
            ui.label(
                eframe::egui::RichText::new(format!(
                    "No {group} component has reported to this panel. That is a gap in what is \
                     being collected, not a clean bill of health."
                ))
                .font(theme::prose())
                .color(theme::DIM),
            );
        });
        return None;
    }

    let mut picked = None;
    ScrollArea::vertical()
        .id_salt(format!("diag-{group}"))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for d in &rows {
                if card::draw(ui, d, now) {
                    picked = Some(d.component.clone());
                }
            }
        });
    picked
}

/// The board's headline: the worst thing on it, then how many rows are in each state.
fn summary(ui: &mut Ui, all: &[Diagnostic], group: &str, title: &str) {
    let counts = order::tally(all, group);
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    widgets::section_count(ui, title, total, theme::DIM);

    widgets::card(ui, 78.0, widgets::Card::default(), |ui| {
        match order::worst_on(all, group) {
            Some(worst) => {
                widgets::state_chip(
                    ui,
                    widgets::health_mark(worst),
                    &format!("WORST HERE  {}", widgets::health_label(worst)),
                    widgets::health_color(worst),
                );
            }
            None => {
                widgets::state_chip(ui, widgets::Mark::Dash, "NOTHING REPORTING", theme::UNKNOWN);
            }
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (health, count) in counts {
                if count == 0 {
                    continue;
                }
                widgets::state_chip(
                    ui,
                    widgets::health_mark(health),
                    &format!("{count} {}", widgets::health_label(health)),
                    widgets::health_color(health),
                );
            }
        });
    });
}
