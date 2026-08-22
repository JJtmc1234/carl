//! The front page: the whole army at once, answerable without scrolling.
//!
//! The panel used to open on the conversation with Carl. That answers "what did I last say",
//! which is a reasonable question and is not the one somebody has when they walk up to a
//! screen. This screen answers six, in this order, and every block on it is a way into the
//! screen that can do something about what it is showing.
//!
//! 1. Is the army healthy. The band across the top.
//! 2. What needs JJ. The left column, worst first, and empty when nothing does.
//! 3. What is Carl doing. Top of the middle column.
//! 4. What projects are active. Under it.
//! 5. What are the agents doing. Top of the right column.
//! 6. What changed recently. Under it, and the thing that makes a quiet screen feel alive.
//!
//! Three columns rather than four, because a fourth would make every card too narrow for a
//! sentence, and this screen is mostly sentences.

use eframe::egui::{Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::vitals;

mod activity;
mod attention;
mod banner;
mod console;
mod feed;
mod roster;
pub mod work;

/// The project ordering, shared with the projects screen so the two never disagree about
/// which project is the most urgent.
pub use work::ordered as project_order;

#[cfg(test)]
mod tests;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let v = vitals::read(&app.snapshot);
    banner::draw(app, ui, &v);
    ui.add_space(theme::GAP);

    let gap = theme::GAP + 4.0;
    let total = ui.available_width();
    let column = ((total - gap * 2.0) / 3.0).floor();
    let height = ui.available_height();

    // Each column scrolls inside its own height. `allocate_ui` reserves room and does not clip,
    // so a column with more in it than fits painted straight over whatever was below, which with
    // the workspace open meant cards on top of the workspace header. Scrolling both clips it and
    // leaves the overflow reachable, which dropping it would not.
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        column_of(ui, "attention", column, height, |ui| {
            attention::draw(app, ui, &v)
        });
        ui.add_space(gap);
        column_of(ui, "command", column, height, |ui| {
            console::draw(app, ui);
            ui.add_space(theme::GAP);
            work::draw(app, ui);
        });
        ui.add_space(gap);
        let rest = total - column * 2.0 - gap * 2.0;
        column_of(ui, "army", rest, height, |ui| {
            roster::draw(app, ui);
            ui.add_space(theme::GAP);
            feed::draw(app, ui);
        });
    });
}

/// One column of the overview, clipped to the room it was given and scrolled when it needs more.
fn column_of(ui: &mut Ui, id: &str, width: f32, height: f32, add: impl FnOnce(&mut Ui)) {
    ui.allocate_ui(vec2(width, height), |ui| {
        eframe::egui::ScrollArea::vertical()
            .id_salt(id)
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_width(width);
                ui.vertical(add);
            });
    });
}
