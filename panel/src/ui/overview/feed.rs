//! The recent activity column: what the army actually did, newest at the top.
//!
//! This is the thing that makes an idle screen feel alive, and it does it without inventing
//! anything. A quiet army produces a short feed and says so.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::widgets::{self, Mark};

use super::activity;

/// How far back the column reaches. Enough to cover a few minutes of a busy army without
/// turning the column into a log viewer.
pub const DEPTH: usize = 24;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let beats = activity::recent(&app.snapshot, DEPTH);
    widgets::section_count(ui, "RECENTLY", beats.len(), theme::DIM);

    if beats.is_empty() {
        widgets::fitted_card(ui, widgets::Card::default(), |ui| {
            widgets::state_chip(ui, Mark::Dash, "NOTHING RECORDED YET", theme::UNKNOWN);
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "No journal record, handover, milestone or turn of conversation has reached \
                     this panel.",
                )
                .font(theme::prose())
                .color(theme::DIM),
            );
        });
        return;
    }

    let now = app.snapshot.at;
    ScrollArea::vertical()
        .id_salt("overview-feed")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for beat in &beats {
                widgets::card(
                    ui,
                    62.0,
                    widgets::Card::default().tone(widgets::Tone::Quiet),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                theme::spaced(beat.kind)
                                    .font(theme::label())
                                    .color(beat.color),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&beat.who)
                                    .font(theme::label())
                                    .color(theme::DIM),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(widgets::ago(now.max(beat.at), beat.at))
                                        .font(theme::label())
                                        .color(theme::FAINT),
                                );
                            });
                        });
                        ui.allocate_ui(vec2(ui.available_width(), 22.0), |ui| {
                            ui.add(
                                Label::new(
                                    RichText::new(&beat.what)
                                        .font(theme::prose())
                                        .color(theme::TEXT),
                                )
                                .truncate(),
                            );
                        });
                    },
                );
            }
        });
}
