//! What Carl is doing, on the front page.
//!
//! Not the conversation. The conversation has a screen of its own and duplicating it here
//! would make the overview a second Carl tab. What belongs here is the answer to "what is the
//! chief executive doing right now", which is his state, the last thing he said, and whether
//! he is waiting on an answer.

use eframe::egui::{Align, Label, Layout, RichText, Ui, vec2};

use crate::app::{App, Tab};
use crate::model::{AgentView, Speaker};
use crate::theme;
use crate::ui::widgets::{self, Mark};

pub const HEIGHT: f32 = 232.0;

pub fn draw(app: &mut App, ui: &mut Ui) {
    widgets::section(ui, "CARL");
    let view = app
        .snapshot
        .agent("carl")
        .cloned()
        .unwrap_or_else(|| AgentView::unknown("carl"));
    let waiting = app.snapshot.decisions.len();
    let now = app.snapshot.at;
    let last = app
        .snapshot
        .conversation
        .iter()
        .rev()
        .find(|t| t.from == Speaker::Carl)
        .cloned();

    let response = widgets::card(
        ui,
        HEIGHT,
        widgets::Card::default().attention(waiting > 0),
        |ui| {
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(vec2(13.0, 13.0), eframe::egui::Sense::hover());
                widgets::mark(
                    ui.painter(),
                    rect,
                    widgets::status_mark(view.status),
                    widgets::status_color(view.status),
                );
                ui.add_space(6.0);
                ui.label(
                    theme::spaced("CARL")
                        .font(theme::heading())
                        .color(theme::TEXT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    widgets::state_chip(
                        ui,
                        widgets::status_mark(view.status),
                        view.status.label(),
                        widgets::status_color(view.status),
                    );
                });
            });
            ui.label(
                RichText::new("chief executive, delegates and decides")
                    .font(theme::label())
                    .color(theme::FAINT),
            );
            ui.add_space(8.0);

            let (doing, colour) = super::super::agents::work_line(&view);
            ui.allocate_ui(vec2(ui.available_width(), 42.0), |ui| {
                ui.add(Label::new(RichText::new(doing).font(theme::prose()).color(colour)).wrap());
            });
            ui.add_space(6.0);

            match &last {
                Some(turn) => {
                    ui.horizontal(|ui| {
                        widgets::small(ui, "LAST SAID");
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(widgets::ago(now, turn.at))
                                    .font(theme::label())
                                    .color(theme::FAINT),
                            );
                        });
                    });
                    ui.allocate_ui(vec2(ui.available_width(), 40.0), |ui| {
                        ui.add(
                            Label::new(
                                RichText::new(&turn.text)
                                    .font(theme::prose())
                                    .color(theme::TEXT),
                            )
                            .wrap(),
                        );
                    });
                }
                None => {
                    // Never a blank. An empty box here reads as Carl having gone quiet, when
                    // what is true is that this panel holds nothing from before it opened.
                    ui.label(
                        RichText::new(
                            "Nothing from before this panel opened. The conversation fills as you talk.",
                        )
                        .font(theme::prose())
                        .color(theme::UNKNOWN),
                    );
                }
            }

            ui.add_space(6.0);
            if waiting > 0 {
                widgets::state_chip(
                    ui,
                    Mark::Half,
                    &format!("{waiting} WAITING ON YOUR ANSWER"),
                    theme::ACCENT,
                );
            }
        },
    );

    if response.clicked() {
        app.select_tab(Tab::Carl);
    }
}
