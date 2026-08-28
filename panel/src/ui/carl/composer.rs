//! The pinned input. Always in the same place, always large enough to be the obvious thing.
//!
//! Two acts, one box. Saying something to Carl and setting an objective for the organisation
//! are different commands and the backend takes them as different commands, so the act is
//! picked first and then there is one place to type. The old screen put a box for each at the
//! bottom of an empty pane, which read as two half finished controls rather than as a console.
//!
//! Which act is selected is a fact about this window, so it lives in the context rather than in
//! the state the backend feeds.

use eframe::egui::{Align, Id, Key, Layout, RichText, TextEdit, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::widgets::{self, Mark};

/// How much room the composer takes at the foot of the screen.
pub const HEIGHT: f32 = 132.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Say,
    Objective,
}

impl Act {
    pub fn label(self) -> &'static str {
        match self {
            Act::Say => "SAY TO CARL",
            Act::Objective => "SET AN OBJECTIVE",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Act::Say => "say something to carl",
            Act::Objective => "a new objective for the organisation",
        }
    }

    /// What this act actually does, said out loud, because the two are not interchangeable and
    /// picking the wrong one sends work down the whole chain.
    pub fn consequence(self) -> &'static str {
        match self {
            Act::Say => "Goes to Carl as a message. He answers, and delegates if he decides to.",
            Act::Objective => {
                "Goes to Carl as an objective. He breaks it up and hands it to a department."
            }
        }
    }
}

fn current(ui: &Ui) -> Act {
    ui.data(|d| d.get_temp::<Act>(Id::new("composer-act")))
        .unwrap_or(Act::Say)
}

fn set(ui: &mut Ui, act: Act) {
    ui.data_mut(|d| d.insert_temp(Id::new("composer-act"), act));
}

pub fn draw(app: &mut App, ui: &mut Ui) {
    let act = current(ui);
    let live = app.can_send();
    let mut chosen = act;

    eframe::egui::Frame::none()
        .fill(theme::RAISED)
        .stroke(theme::edge(if live {
            theme::RULE_BRIGHT
        } else {
            theme::BAD
        }))
        .rounding(theme::CARD_CORNER)
        .inner_margin(eframe::egui::Margin::same(theme::PAD))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                for option in [Act::Say, Act::Objective] {
                    let picked = option == act;
                    let text = RichText::new(option.label())
                        .font(theme::label())
                        .color(if picked { theme::VOID } else { theme::DIM });
                    let button = eframe::egui::Button::new(text)
                        .fill(if picked { theme::ACCENT } else { theme::PANEL })
                        .stroke(theme::edge(if picked {
                            theme::ACCENT
                        } else {
                            theme::RULE
                        }));
                    if ui.add(button).clicked() {
                        chosen = option;
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !live {
                        widgets::state_chip(ui, Mark::Cross, "CANNOT SEND, LINK DOWN", theme::BAD);
                    }
                });
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(act.consequence())
                    .font(theme::label())
                    .color(theme::FAINT),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                let send_width = 118.0;
                let box_width = (ui.available_width() - send_width - 10.0).max(120.0);
                let buffer = match act {
                    Act::Say => &mut app.draft,
                    Act::Objective => &mut app.objective,
                };
                let response = ui.add_sized(
                    vec2(box_width, 42.0),
                    TextEdit::singleline(buffer)
                        .font(theme::prose())
                        .hint_text(act.hint())
                        .margin(eframe::egui::Margin::symmetric(12.0, 12.0)),
                );
                let entered = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let pressed = ui
                    .add_sized(
                        vec2(send_width, 42.0),
                        eframe::egui::Button::new(
                            RichText::new("SEND").font(theme::body()).color(if live {
                                theme::VOID
                            } else {
                                theme::FAINT
                            }),
                        )
                        .fill(if live {
                            theme::ACCENT
                        } else {
                            theme::PANEL
                        }),
                    )
                    .clicked();
                if pressed || entered {
                    match act {
                        Act::Say => app.send_draft(),
                        Act::Objective => app.send_objective(),
                    }
                    response.request_focus();
                }
            });
        });

    if chosen != act {
        set(ui, chosen);
    }
}
