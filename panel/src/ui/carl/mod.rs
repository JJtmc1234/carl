//! JJ's command console. A conversation, an input that is always there, and the organisation
//! working alongside it.
//!
//! The old version of this screen had two small text boxes floating at the bottom of a mostly
//! empty pane and a chat log above them with no edges. It worked and it looked like a debug
//! REPL. What a console needs is for the conversation to be a surface you are in, for the
//! input to be unmissable and always in the same place, for the things Carl cannot settle to
//! interrupt without taking the screen, and for what the organisation is doing to be visible
//! beside it so an answer can be given with the context in view.
//!
//! One input, not two. Sending a message and setting an objective are genuinely different acts
//! and the backend takes them as different commands, so the composer has a mode rather than a
//! second box: the act is chosen, and then there is one place to type.

use eframe::egui::{RichText, Ui, vec2};

use crate::app::App;
use crate::theme;

use super::{shell, widgets};

mod beside;
mod composer;
mod conversation;
mod decisions;

#[cfg(test)]
mod tests;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.68);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| {
                let asked = decisions::draw(app, ui);
                let composer_height = composer::HEIGHT;
                let remaining = (ui.available_height() - composer_height - theme::GAP).max(200.0);
                ui.allocate_ui(vec2(ui.available_width(), remaining), |ui| {
                    conversation::draw(app, ui, asked);
                });
                ui.add_space(theme::GAP);
                composer::draw(app, ui);
            });
        });
        ui.add_space(theme::GAP + 4.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| beside::draw(app, ui));
        });
    });
}

/// A short line that says why an empty area is empty, used by more than one block here.
pub fn nothing_yet(ui: &mut Ui, headline: &str, why: &str) {
    widgets::state_chip(ui, widgets::Mark::Dash, headline, theme::UNKNOWN);
    ui.add_space(6.0);
    ui.label(RichText::new(why).font(theme::prose()).color(theme::DIM));
}
