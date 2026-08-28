//! The frame everything else sits in.
//!
//! A persistent left rail, a top status strip, a full width band the moment the link goes, the
//! screen underneath, and the contextual workspace docked at the bottom when something has
//! opened it.
//!
//! The rail is the thing that is true regardless of which screen you are on: what the panel is
//! attached to, whether that attachment is live, how the army is doing, and how many things
//! want somebody. It is never hidden and it never scrolls away, because the one question a
//! command centre must answer without being asked is whether what is on it can be believed.

use eframe::egui::{CentralPanel, Context, RichText, Sense, TopBottomPanel, vec2};

use crate::app::{App, Tab};
use crate::theme;

mod asking;
mod rail;
mod strip;

#[cfg(test)]
mod tests;

pub use rail::wants_attention;

pub fn draw(app: &mut App, ctx: &Context) {
    rail::draw(app, ctx);
    strip::draw(app, ctx);
    warning(app, ctx);
    // Below the not live band on purpose. If the link is down these buttons cannot send, and
    // the reason why has to be the first thing read.
    asking::draw(app, ctx);
    if app.workspace.is_some() {
        super::workspace::draw(app, ctx);
    }
    CentralPanel::default()
        .frame(eframe::egui::Frame::none().fill(theme::VOID).inner_margin(
            eframe::egui::Margin::symmetric(theme::GAP + 4.0, theme::GAP),
        ))
        .show(ctx, |ui| match app.tab {
            Tab::Overview => super::overview::draw(app, ui),
            Tab::Carl => super::carl::draw(app, ui),
            Tab::Agents => super::agents::draw(app, ui),
            Tab::Diagnostics => super::diagnostics::draw(app, ui),
            Tab::Projects => super::projects::draw(app, ui),
        });
}

/// The band that appears the moment the panel stops being live.
///
/// Its own full width strip rather than a phrase squeezed into the top bar. Everything below
/// it is a version of the world from before the link went, and the one thing a panel must
/// never do is let that look current, so it gets a whole line of the screen and a colour used
/// for nothing else up there.
fn warning(app: &mut App, ctx: &Context) {
    let Some(text) = stale_warning(app) else {
        return;
    };
    TopBottomPanel::top("not-live")
        .exact_height(38.0)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::BAD.linear_multiply(0.18))
                .stroke(theme::edge(theme::BAD))
                .inner_margin(eframe::egui::Margin::symmetric(16.0, 9.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                super::widgets::state_chip(ui, super::widgets::Mark::Cross, "NOT LIVE", theme::BAD);
                ui.add_space(10.0);
                ui.label(RichText::new(text).font(theme::prose()).color(theme::TEXT));
            });
        });
}

/// The sentence that appears the moment the panel stops being live.
pub fn stale_warning(app: &App) -> Option<String> {
    match &app.link {
        crate::model::Link::Live => None,
        crate::model::Link::Connecting { attempt } => Some(format!(
            "everything below is from before the link dropped, reconnecting {attempt}"
        )),
        crate::model::Link::Disconnected { why } => Some(format!(
            "everything below is from before the link dropped, {why}"
        )),
    }
}

/// Splits the space into two columns with the standard gap between them.
pub fn columns_for(ui: &mut eframe::egui::Ui, left_fraction: f32) -> (f32, f32) {
    let total = ui.available_width();
    let gap = theme::GAP + 4.0;
    let left = ((total - gap) * left_fraction).floor();
    (left, total - gap - left)
}

/// A clickable area that reads as a link rather than a button, for the many places the panel
/// offers to open something in the workspace.
pub fn open_link(ui: &mut eframe::egui::Ui, text: &str) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::body(), theme::COLD);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x, galley.size().y + 4.0), Sense::click());
    let color = if response.hovered() {
        theme::ACCENT
    } else {
        theme::COLD
    };
    ui.painter().galley(rect.left_top(), galley, color);
    ui.painter().hline(
        rect.left()..=rect.right(),
        rect.bottom() - 1.0,
        theme::edge(if response.hovered() {
            color
        } else {
            theme::RULE_BRIGHT
        }),
    );
    response.clicked()
}
