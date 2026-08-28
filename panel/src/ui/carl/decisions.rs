//! Anything Carl cannot settle himself.
//!
//! A band above the conversation rather than a modal, and never more than a band. It has to be
//! impossible to miss and must not take the screen, because the conversation underneath is
//! usually the context somebody needs in order to answer.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::widgets::{self, Mark};

/// The tallest the band is allowed to get, however many questions are waiting. Past this it
/// scrolls, so three questions cannot push the conversation off the screen.
pub const MOST: f32 = 320.0;

/// Draws the band and says how many questions are in it, so the block below knows whether the
/// screen is in an interrupted state.
pub fn draw(app: &mut App, ui: &mut Ui) -> usize {
    let pending: Vec<_> = app.snapshot.decisions.clone();
    if pending.is_empty() {
        return 0;
    }

    let one = 160.0_f32;
    let height = (one * pending.len() as f32).min(MOST);
    let mut answer: Option<(String, String)> = None;

    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
        ScrollArea::vertical()
            .id_salt("decisions")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for decision in &pending {
                    widgets::fitted_card(ui, widgets::Card::default().attention(true), |ui| {
                        ui.horizontal(|ui| {
                            widgets::state_chip(ui, Mark::Half, "CARL NEEDS YOU", theme::ACCENT);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(widgets::ago(app.snapshot.at, decision.asked_at))
                                        .font(theme::label())
                                        .color(theme::FAINT),
                                );
                            });
                        });
                        ui.add_space(4.0);
                        ui.add(
                            Label::new(
                                RichText::new(&decision.question)
                                    .font(theme::prose())
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                        if let Some(detail) = &decision.detail {
                            ui.add_space(3.0);
                            ui.add(
                                Label::new(
                                    RichText::new(detail).font(theme::prose()).color(theme::DIM),
                                )
                                .wrap(),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            for option in &decision.options {
                                if ui
                                    .button(RichText::new(option).font(theme::body()))
                                    .clicked()
                                {
                                    answer = Some((decision.id.clone(), option.clone()));
                                }
                            }
                            ui.label(
                                RichText::new("or answer in your own words below")
                                    .font(theme::label())
                                    .color(theme::FAINT),
                            );
                        });
                    });
                    ui.add_space(10.0);
                }
            });
    });
    ui.add_space(theme::GAP);

    if let Some((id, option)) = answer {
        app.answer_decision(&id, &option);
    }
    pending.len()
}
