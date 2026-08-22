//! What needs JJ, worst first, and nothing else.
//!
//! The hardest column to get right, because the temptation is to keep it looking busy. A list
//! that always has rows in it is a list somebody stops reading, so when the army wants nothing
//! this says so plainly and stays empty. The empty state is a real one: it says what was
//! checked, which is the difference between "all clear" and "nothing has been looked at".

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::vitals::{self, Need, Vitals};
use crate::ui::widgets::{self, Mark};

pub fn draw(app: &mut App, ui: &mut Ui, v: &Vitals) {
    let needs = vitals::needs(&app.snapshot);
    widgets::section_count(
        ui,
        "WHAT NEEDS YOU",
        needs.len(),
        if needs.is_empty() {
            theme::FAINT
        } else {
            theme::ACCENT
        },
    );

    if needs.is_empty() {
        quiet(ui, v);
        return;
    }

    let mut go: Option<crate::app::Tab> = None;
    let mut pick: Option<String> = None;
    ScrollArea::vertical()
        .id_salt("overview-needs")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for need in &needs {
                if row(ui, need) {
                    go = Some(need.goes_to);
                    pick = Some(need.subject.clone());
                }
            }
        });

    if let (Some(tab), Some(subject)) = (go, pick) {
        follow(app, tab, &subject);
    }
}

fn row(ui: &mut Ui, need: &Need) -> bool {
    let response = widgets::fitted_card(ui, widgets::Card::default().attention(true), |ui| {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(11.0, 11.0), eframe::egui::Sense::hover());
            widgets::mark(ui.painter(), rect, need.mark, need.color);
            ui.add_space(4.0);
            ui.label(
                theme::spaced(need.kind)
                    .font(theme::label())
                    .color(need.color),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(&need.subject)
                        .font(theme::body())
                        .color(theme::TEXT),
                );
            });
        });
        ui.add_space(2.0);
        ui.allocate_ui(vec2(ui.available_width(), 46.0), |ui| {
            ui.add(
                Label::new(
                    RichText::new(&need.detail)
                        .font(theme::prose())
                        .color(theme::TEXT),
                )
                .wrap(),
            );
        });
    });
    response.clicked()
}

/// Where clicking one of these lands, which is always the screen that can do something about
/// it rather than a dialog that repeats what the row already said.
fn follow(app: &mut App, tab: crate::app::Tab, subject: &str) {
    match tab {
        crate::app::Tab::Agents => app.select_agent(subject),
        crate::app::Tab::Projects => app.select_project(subject),
        _ => {}
    }
    app.select_tab(tab);
}

/// Nothing wants anybody. Said in a way that also says what that claim covers.
fn quiet(ui: &mut Ui, v: &Vitals) {
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
        widgets::state_chip(ui, Mark::Hollow, "NOTHING IS WAITING ON YOU", theme::FAINT);
        ui.add_space(8.0);
        ui.label(
            RichText::new(format!(
                "No question from Carl, nobody blocked, no component failed. \
                 That covers {} agents and {} components.",
                v.agents(),
                v.components()
            ))
            .font(theme::prose())
            .color(theme::DIM),
        );
        if v.unmeasured > 0 {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!(
                    "{} component(s) have never been read, so they are outside that claim.",
                    v.unmeasured
                ))
                .font(theme::prose())
                .color(theme::UNKNOWN),
            );
        }
    });
}
