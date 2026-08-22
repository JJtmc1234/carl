//! The band across the top of the overview: is the army healthy, and how big is it.
//!
//! The one place in the interface where type is allowed to be large. Somebody who has just
//! walked up to the screen reads this and nothing else, so it says the worst true thing in
//! words, marks it with a shape, and puts the counts that back it up underneath.

use eframe::egui::{Align, Layout, RichText, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::vitals::Vitals;
use crate::ui::widgets::{self, Mark};

pub const HEIGHT: f32 = 176.0;
/// One stat tile. Wide enough for the longest word under it, which is UNMEASURED.
const TILE: f32 = 104.0;

pub fn draw(app: &App, ui: &mut Ui, v: &Vitals) {
    let (word, colour, shape) = v.headline();
    let alarming = v.wants_jj() > 0;

    widgets::fitted_card(ui, widgets::Card::default().attention(alarming), |ui| {
        // Explicit widths for all three blocks. A vertical stack inside a horizontal row
        // is handed whatever is left over, and a spaced label with nothing left over wraps
        // to one character per line, which is exactly what this band did before.
        let total = ui.available_width();
        // Three columns need room. Below this they do not shrink gracefully, they run off
        // the right hand edge of the card, which is what DEGRADED was doing at 1280. So a
        // narrow window stacks them instead. Boring, and it cannot overflow.
        const THREE_UP: f32 = 1120.0;

        if total < THREE_UP {
            headline(ui, word, colour, shape, v);
            ui.add_space(12.0);
            people(ui, v);
            ui.add_space(12.0);
            components(ui, v);
        } else {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let people_width = (TILE * 5.0).min(total * 0.34);
                let component_width = (TILE * 4.0).min(total * 0.28);
                let headline_width = (total - people_width - component_width - 48.0).max(280.0);

                ui.allocate_ui_with_layout(
                    vec2(headline_width, HEIGHT - 78.0),
                    Layout::top_down(Align::Min),
                    |ui| headline(ui, word, colour, shape, v),
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    vec2(people_width, HEIGHT - 78.0),
                    Layout::top_down(Align::Min),
                    |ui| people(ui, v),
                );
                ui.add_space(24.0);
                ui.allocate_ui_with_layout(
                    vec2(component_width, HEIGHT - 78.0),
                    Layout::top_down(Align::Min),
                    |ui| components(ui, v),
                );
            });
        }
        ui.add_space(6.0);
        // Wrapped, because this sentence grows with how many things could not be measured
        // and a narrow window is where it first runs off the end.
        ui.add(
            eframe::egui::Label::new(
                RichText::new(footing(app, v))
                    .font(theme::prose())
                    .color(theme::DIM),
            )
            .wrap(),
        );
    });
}

fn headline(ui: &mut Ui, word: &str, colour: eframe::egui::Color32, shape: Mark, v: &Vitals) {
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), eframe::egui::Sense::hover());
            widgets::mark(ui.painter(), rect, shape, colour);
            ui.add_space(6.0);
            ui.label(theme::spaced(word).font(theme::title()).color(colour));
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(format!(
                "{} agents in the chain, {} components reporting",
                v.agents(),
                v.components()
            ))
            .font(theme::prose())
            .color(theme::DIM),
        );
        ui.add_space(8.0);
        if v.wants_jj() > 0 {
            widgets::state_chip(
                ui,
                Mark::Barred,
                &format!("{} THINGS WANT YOU", v.wants_jj()),
                theme::ACCENT,
            );
        } else {
            widgets::state_chip(ui, Mark::Hollow, "NOTHING WANTS YOU", theme::FAINT);
        }
    });
}

/// One number, large, with the word for it underneath. Never a number on its own.
fn tile(ui: &mut Ui, count: usize, name: &str, colour: eframe::egui::Color32, shape: Mark) {
    let quiet = count == 0;
    let ink = if quiet { theme::FAINT } else { colour };

    // Measured, not assumed. TILE was a constant "wide enough for the longest word", which was
    // true when it was written and stopped being true the moment the type scale grew: DEGRADED
    // ran off the right hand edge of the card at a narrow window. Asking the font how wide the
    // word actually is cannot go stale.
    let caption = theme::spaced(name).font(theme::label());
    let needed = ui.fonts(|f| {
        f.layout_no_wrap(
            caption.text().to_string(),
            theme::label(),
            eframe::egui::Color32::WHITE,
        )
        .size()
        .x
    }) + 9.0
        + 2.0
        + theme::PAD;

    let width = needed.max(TILE - 8.0);
    ui.vertical(|ui| {
        ui.set_min_width(width);
        ui.set_max_width(width);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(count.to_string())
                    .font(theme::display())
                    .color(if quiet { theme::FAINT } else { theme::TEXT }),
            );
        });
        ui.add_space(-6.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(9.0, 9.0), eframe::egui::Sense::hover());
            widgets::mark(ui.painter(), rect, shape, ink);
            ui.add_space(2.0);
            ui.label(caption.color(ink));
        });
    });
}

fn people(ui: &mut Ui, v: &Vitals) {
    ui.vertical(|ui| {
        widgets::small(ui, "THE ARMY");
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            tile(ui, v.working, "WORKING", theme::ACCENT, Mark::Filled);
            tile(ui, v.review, "REVIEW", theme::COLD, Mark::Half);
            tile(ui, v.blocked, "BLOCKED", theme::BAD, Mark::Barred);
            tile(ui, v.idle, "IDLE", theme::FAINT, Mark::Hollow);
            tile(ui, v.unknown, "UNKNOWN", theme::UNKNOWN, Mark::Dash);
        });
    });
}

fn components(ui: &mut Ui, v: &Vitals) {
    ui.vertical(|ui| {
        widgets::small(ui, "COMPONENTS");
        // Wrapped rather than a fixed row. Four tiles at their natural width need more room
        // than a narrow window gives this block, and a row that cannot fit does not shrink,
        // it runs off the end of the card.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            tile(ui, v.failed, "FAILED", theme::BAD, Mark::Cross);
            tile(ui, v.degraded + v.held, "DEGRADED", theme::WARN, Mark::Half);
            // Never folded into healthy. A component nothing has read is a gap, and a gap that
            // is counted as a pass is the one number on this screen that could get somebody
            // hurt.
            tile(ui, v.unmeasured, "UNMEASURED", theme::UNKNOWN, Mark::Dash);
            tile(ui, v.healthy, "HEALTHY", theme::GOOD, Mark::Filled);
        });
    });
}

/// The sentence under the numbers, which is where the panel says what it is attached to.
///
/// Repeated here as well as in the rail on purpose. This band is the thing somebody reads from
/// across the room, and a headline about the health of an army that is not the real army has
/// to carry that fact with it.
fn footing(app: &App, v: &Vitals) -> String {
    let source = app.source_name();
    let where_from = if source.contains("mock") {
        format!("scripted mock, not the real army, {source}")
    } else {
        format!("live backend, {source}")
    };
    if v.unmeasured > 0 {
        format!(
            "{where_from}. {} component(s) have no reading, so this headline covers only what was measured.",
            v.unmeasured
        )
    } else {
        format!("{where_from}. Every component reporting has a reading.")
    }
}
