//! JJ's command surface. A conversation first, and only then anything else.
//!
//! The layout says what this tab is for: the conversation takes the width it needs and the
//! rest of the organisation is a narrow column beside it. A pending decision is the one thing
//! allowed to interrupt, and it sits above the conversation as a band rather than a modal,
//! because the answer usually depends on what was said just underneath it.

use eframe::egui::{Align, Key, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};

use crate::app::App;
use crate::model::Speaker;
use crate::theme;

use super::{shell, widgets};

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.66);

    ui.horizontal_top(|ui| {
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| {
                decisions(app, ui);
                conversation(app, ui, left);
                composer(app, ui);
            });
        });
        ui.add_space(16.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| beside(app, ui));
        });
    });
}

/// Anything Carl cannot settle himself.
///
/// Drawn in the accent, at the top, and never more than a band. It has to be impossible to
/// miss and must not take the screen, because the conversation under it is usually the context
/// somebody needs in order to answer.
fn decisions(app: &mut App, ui: &mut Ui) {
    let pending: Vec<_> = app.snapshot.decisions.clone();
    if pending.is_empty() {
        return;
    }

    for decision in pending {
        eframe::egui::Frame::none()
            .fill(theme::RAISED)
            .stroke(theme::edge(theme::ACCENT))
            .rounding(theme::CORNER)
            .inner_margin(eframe::egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(theme::spaced("CARL NEEDS YOU"))
                        .font(theme::label())
                        .color(theme::ACCENT),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&decision.question)
                        .font(theme::body())
                        .color(theme::TEXT),
                );
                if let Some(detail) = &decision.detail {
                    ui.add_space(3.0);
                    ui.label(RichText::new(detail).font(theme::label()).color(theme::DIM));
                }
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    for option in &decision.options {
                        if ui
                            .button(RichText::new(option).font(theme::label()))
                            .clicked()
                        {
                            app.answer_decision(&decision.id, option);
                        }
                    }
                    ui.label(
                        RichText::new("or answer below")
                            .font(theme::label())
                            .color(theme::FAINT),
                    );
                });
            });
        ui.add_space(8.0);
    }
}

fn conversation(app: &mut App, ui: &mut Ui, width: f32) {
    let height = (ui.available_height() - 132.0).max(160.0);
    let now = app.snapshot.at;

    ScrollArea::vertical()
        .id_salt("conversation")
        .stick_to_bottom(app.conversation_at_end)
        .max_height(height)
        .show(ui, |ui| {
            ui.set_width(width);
            for turn in &app.snapshot.conversation {
                let (who, color) = match turn.from {
                    Speaker::Jj => ("JJ", theme::COLD),
                    Speaker::Carl => ("CARL", theme::ACCENT),
                };
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(theme::spaced(who))
                            .font(theme::label())
                            .color(color),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(widgets::ago(now, turn.at))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    });
                });
                ui.add_space(2.0);
                let text = if turn.streaming {
                    // A caret while the words are still arriving, so a half written answer is
                    // never mistaken for a finished one.
                    format!("{}\u{2588}", turn.text)
                } else {
                    turn.text.clone()
                };
                ui.label(RichText::new(text).font(theme::body()).color(theme::TEXT));
                ui.add_space(12.0);
            }
        });
}

/// The message box and the objective box, which are two different acts.
fn composer(app: &mut App, ui: &mut Ui) {
    widgets::rule(ui);
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let send_width = 74.0;
        let box_width = ui.available_width() - send_width - 10.0;
        let response = ui.add_sized(
            vec2(box_width, 30.0),
            TextEdit::singleline(&mut app.draft)
                .font(theme::body())
                .hint_text("say something to carl"),
        );
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if ui
            .add_sized(vec2(send_width, 30.0), eframe::egui::Button::new("SEND"))
            .clicked()
            || entered
        {
            app.send_draft();
            response.request_focus();
        }
    });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let button_width = 100.0;
        let box_width = ui.available_width() - button_width - 10.0;
        ui.add_sized(
            vec2(box_width, 26.0),
            TextEdit::singleline(&mut app.objective)
                .font(theme::label())
                .hint_text("new objective for the organisation"),
        );
        if ui
            .add_sized(
                vec2(button_width, 26.0),
                eframe::egui::Button::new(RichText::new("SET OBJECTIVE").font(theme::label())),
            )
            .clicked()
        {
            app.send_objective();
        }
    });
}

/// The narrow column: what Carl has been doing with what he was given.
///
/// Kept deliberately thin. This tab is for talking to Carl, and the moment this column starts
/// growing it has become a dashboard with a chat box in the corner.
fn beside(app: &mut App, ui: &mut Ui) {
    let now = app.snapshot.at;

    widgets::section(ui, "IN HAND");
    let working: Vec<_> = app
        .snapshot
        .agents
        .iter()
        .filter(|a| a.status != crate::model::AgentStatus::Idle && a.name != "jj")
        .cloned()
        .collect();

    if working.is_empty() {
        ui.label(
            RichText::new("nobody is working")
                .font(theme::label())
                .color(theme::FAINT),
        );
    }
    for agent in working {
        ui.horizontal(|ui| {
            widgets::pip(
                ui,
                widgets::status_color(agent.status),
                agent.status != crate::model::AgentStatus::Unknown,
            );
            ui.label(
                RichText::new(&agent.name)
                    .font(theme::body())
                    .color(theme::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(agent.status.label())
                        .font(theme::label())
                        .color(widgets::status_color(agent.status)),
                );
            });
        });
        if let Some(activity) = &agent.last_activity {
            ui.label(
                RichText::new(activity)
                    .font(theme::label())
                    .color(theme::DIM),
            );
        }
        ui.add_space(8.0);
    }

    ui.add_space(10.0);
    widgets::section(ui, "RECENT DELEGATIONS");
    let recent: Vec<_> = app
        .snapshot
        .delegations
        .iter()
        .rev()
        .take(5)
        .cloned()
        .collect();
    if recent.is_empty() {
        ui.label(
            RichText::new("nothing handed down yet")
                .font(theme::label())
                .color(theme::FAINT),
        );
    }
    for d in recent {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} to {}", d.from, d.to))
                    .font(theme::label())
                    .color(theme::COLD),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(widgets::ago(now, d.at))
                        .font(theme::label())
                        .color(theme::FAINT),
                );
            });
        });
        ui.label(
            RichText::new(&d.goal)
                .font(theme::label())
                .color(theme::DIM),
        );
        ui.add_space(8.0);
    }
}
