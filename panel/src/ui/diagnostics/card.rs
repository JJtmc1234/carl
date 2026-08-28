//! One component, on one card.
//!
//! The metrics are the part that used to collide. They are laid out in fixed columns worked
//! out from the card's own width, and each cell is truncated to its column, so two readings
//! can never overlap however long their names are. Aligned columns are also what makes a board
//! of ten components readable down the page rather than left to right one row at a time.

use eframe::egui::{Align, Label, Layout, RichText, Ui, vec2};

use crate::model::{Diagnostic, Health, stale};
use crate::theme;
use crate::ui::widgets;

use super::order::short_name;

/// How many metric columns a card of this width can carry without cramping.
pub fn columns_for(width: f32) -> usize {
    match width {
        w if w >= 760.0 => 4,
        w if w >= 520.0 => 3,
        w if w >= 320.0 => 2,
        _ => 1,
    }
}

/// How tall a card with this many metrics has to be.
pub fn height_for(metrics: usize, columns: usize) -> f32 {
    let rows = metrics.div_ceil(columns.max(1));
    92.0 + rows as f32 * 24.0
}

/// Draws one component. Returns true when somebody asked to look into it.
pub fn draw(ui: &mut Ui, d: &Diagnostic, now: u64) -> bool {
    let is_stale = stale(d, now);
    let colour = widgets::health_color(d.health);
    let pairs = d.metric_pairs();
    let columns = columns_for(ui.available_width());
    let height = height_for(pairs.len(), columns);

    let response = widgets::card(
        ui,
        height,
        widgets::Card::default().attention(widgets::wants_attention(d.health)),
        |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(12.0, 12.0), eframe::egui::Sense::hover());
                widgets::mark(ui.painter(), rect, widgets::health_mark(d.health), colour);
                ui.add_space(5.0);
                ui.add(
                    Label::new(
                        RichText::new(short_name(&d.component))
                            .font(theme::heading())
                            .color(theme::TEXT),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::state_chip(
                        ui,
                        widgets::health_mark(d.health),
                        widgets::health_label(d.health),
                        colour,
                    );
                });
            });

            ui.horizontal(|ui| {
                // A stale sample is dimmed rather than hidden, so it is visibly old instead of
                // quietly passing as current.
                let summary_colour = if is_stale { theme::UNKNOWN } else { theme::DIM };
                let freshness = widgets::freshness(d, now);
                let reserve = if freshness.is_some() { 150.0 } else { 0.0 };
                ui.allocate_ui(
                    vec2((ui.available_width() - reserve).max(80.0), 22.0),
                    |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(&d.summary)
                                    .font(theme::prose())
                                    .color(summary_colour),
                            )
                            .truncate(),
                        );
                    },
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(f) = freshness {
                        ui.label(RichText::new(f).font(theme::label()).color(if is_stale {
                            theme::WARN
                        } else {
                            theme::FAINT
                        }));
                    }
                });
            });

            ui.add_space(4.0);
            metrics(ui, &pairs, columns, d.health);
        },
    );

    response.clicked()
}

/// The readings, in fixed columns.
///
/// `metric_pairs` renders an unreadable value as the word unknown rather than as an empty
/// string, so a gap can never be mistaken for a zero. What this adds is that the gap lands in
/// the same column as every other value, which is what makes a column of readings comparable
/// down the page.
fn metrics(ui: &mut Ui, pairs: &[(String, String)], columns: usize, health: Health) {
    if pairs.is_empty() {
        ui.label(
            RichText::new(if health == Health::Unknown {
                "no reading was taken"
            } else {
                "no figures reported for this component"
            })
            .font(theme::label())
            .color(theme::UNKNOWN),
        );
        return;
    }

    let cell = (ui.available_width() / columns as f32).floor().max(80.0);
    for row in pairs.chunks(columns) {
        ui.horizontal(|ui| {
            // Set here rather than inherited. The screen zeroes horizontal spacing so its two
            // columns can meet exactly, and a figure row that inherits that runs its cells
            // together: "free 816 GiBtotal 981 GiB" was one reading pretending to be two.
            ui.spacing_mut().item_spacing.x = 18.0;
            for (key, value) in row {
                ui.allocate_ui(vec2(cell - 8.0, 22.0), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 5.0;
                        ui.add(
                            Label::new(RichText::new(key).font(theme::label()).color(theme::FAINT))
                                .truncate(),
                        );
                        // A value that could not be read says so, in the colourless grey, so it
                        // never passes as a measurement.
                        let unreadable = value.to_lowercase().contains("unknown");
                        ui.add(
                            Label::new(RichText::new(value).font(theme::body()).color(
                                if unreadable {
                                    theme::UNKNOWN
                                } else {
                                    theme::TEXT
                                },
                            ))
                            .truncate(),
                        );
                    });
                });
            }
        });
    }
}

#[cfg(test)]
mod tests;
