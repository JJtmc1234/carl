//! The one place JJ goes around the chain.
//!
//! Drawn in a colour used nowhere else and separated from everything above it, so it cannot be
//! done while believing you were sending an ordinary message.

use eframe::egui::{RichText, TextEdit, Ui, vec2};

use crate::app::App;
use crate::command::InterventionKind;
use crate::theme;
use crate::ui::widgets::{self, Mark};

pub fn draw(app: &mut App, ui: &mut Ui, agent: &str) {
    eframe::egui::Frame::none()
        .fill(theme::PANEL)
        .stroke(theme::edge(theme::INTERVENE))
        .rounding(theme::CARD_CORNER)
        .inner_margin(eframe::egui::Margin::same(theme::PAD))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            widgets::state_chip(ui, Mark::Cross, "DIRECT JJ INTERVENTION", theme::INTERVENE);
            ui.add_space(6.0);
            // Carl reports to JJ, so "around JJ" describes JJ going around himself. Only say
            // what is being bypassed when there is actually somebody in between.
            let lead = carl::army::org::find(agent).and_then(|a| a.reports_to);
            let line = match lead {
                Some("jj") | None => format!(
                    "Goes straight to {agent}, who already answers to you. Recorded as JJ acting directly."
                ),
                Some(lead) => format!(
                    "Goes straight to {agent} and around {lead}. Recorded as JJ acting directly."
                ),
            };
            ui.label(RichText::new(line).font(theme::prose()).color(theme::DIM));
            ui.add_space(10.0);

            ui.horizontal_wrapped(|ui| {
                for kind in InterventionKind::ALL {
                    let active = app
                        .intervening
                        .as_ref()
                        .is_some_and(|i| i.kind == kind && i.agent == agent);
                    let text = RichText::new(kind.label())
                        .font(theme::label())
                        .color(if active { theme::VOID } else { theme::DIM });
                    let button = eframe::egui::Button::new(text)
                        .fill(if active {
                            theme::INTERVENE
                        } else {
                            theme::PANEL
                        })
                        .stroke(theme::edge(if active {
                            theme::INTERVENE
                        } else {
                            theme::RULE
                        }));
                    if ui.add(button).clicked() {
                        app.select_agent(agent);
                        app.begin_intervention(kind);
                    }
                }
            });

            let composing = app
                .intervening
                .as_ref()
                .filter(|i| i.agent == agent)
                .cloned();
            let Some(current) = composing else {
                return;
            };

            ui.add_space(10.0);
            widgets::small(ui, &current.kind.prompt().to_uppercase());
            if let Some(i) = app.intervening.as_mut() {
                ui.add_sized(
                    vec2(ui.available_width(), 76.0),
                    TextEdit::multiline(&mut i.body)
                        .font(theme::prose())
                        .hint_text(current.kind.prompt()),
                );
            }

            if let Some(warning) = app.intervention_warning() {
                ui.add_space(8.0);
                ui.label(
                    RichText::new(warning)
                        .font(theme::prose())
                        .color(theme::INTERVENE),
                );
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let confirming = app.intervening.as_ref().is_some_and(|i| i.confirming);
                let label = if confirming {
                    "CONFIRM AND SEND"
                } else if current.kind.is_forceful() {
                    "REVIEW"
                } else {
                    "SEND"
                };
                let ready = app.intervention_ready();
                if ui
                    .add_enabled(
                        ready,
                        eframe::egui::Button::new(
                            RichText::new(label).font(theme::body()).color(if ready {
                                theme::VOID
                            } else {
                                theme::FAINT
                            }),
                        )
                        .fill(if ready {
                            theme::INTERVENE
                        } else {
                            theme::PANEL
                        }),
                    )
                    .clicked()
                {
                    app.send_intervention();
                }
                if ui
                    .button(RichText::new("CANCEL").font(theme::body()))
                    .clicked()
                {
                    app.cancel_intervention();
                }
            });
        });
}
