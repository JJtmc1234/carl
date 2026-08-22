//! What the organisation is doing while you talk to it.
//!
//! Deliberately a column and not a dashboard. This screen is for talking to Carl, and the
//! moment this side starts growing it has become a dashboard with a chat box in the corner,
//! which is what the overview is for.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::{App, Tab};
use crate::model::AgentStatus;
use crate::theme;
use crate::ui::widgets;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let now = app.snapshot.at;
    let height = ui.available_height();

    ui.allocate_ui(vec2(ui.available_width(), (height * 0.5).floor()), |ui| {
        ui.vertical(|ui| in_hand(app, ui));
    });
    ui.add_space(theme::GAP);
    delegations(app, ui, now);
}

/// Who is actually moving right now.
fn in_hand(app: &mut App, ui: &mut Ui) {
    let working: Vec<_> = app
        .snapshot
        .agents
        .iter()
        .filter(|a| a.status != AgentStatus::Idle && !crate::ui::vitals::is_human(&a.name))
        .cloned()
        .collect();
    widgets::section_count(ui, "IN HAND", working.len(), theme::DIM);

    if working.is_empty() {
        widgets::fitted_card(ui, widgets::Card::default(), |ui| {
            super::nothing_yet(
                ui,
                "NOBODY IS WORKING",
                "Every agent is idle. Say something, or set an objective, and Carl will hand it \
                 down.",
            );
        });
        return;
    }

    let mut pick: Option<String> = None;
    ScrollArea::vertical()
        .id_salt("carl-in-hand")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for agent in &working {
                let response = widgets::card(
                    ui,
                    68.0,
                    widgets::Card::default()
                        .attention(agent.status.wants_attention())
                        .lit(app.is_lit(&agent.name)),
                    |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(10.0, 10.0),
                                eframe::egui::Sense::hover(),
                            );
                            widgets::mark(
                                ui.painter(),
                                rect,
                                widgets::status_mark(agent.status),
                                widgets::status_color(agent.status),
                            );
                            ui.add_space(4.0);
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
                        let (text, colour) = crate::ui::agents::work_line(agent);
                        ui.add(
                            Label::new(RichText::new(text).font(theme::prose()).color(colour))
                                .truncate(),
                        );
                    },
                );
                if response.clicked() {
                    pick = Some(agent.name.clone());
                }
            }
        });

    if let Some(name) = pick {
        app.select_agent(&name);
        app.select_tab(Tab::Agents);
    }
}

/// What has been handed down, most recent first.
fn delegations(app: &mut App, ui: &mut Ui, now: u64) {
    let recent: Vec<_> = app
        .snapshot
        .delegations
        .iter()
        .rev()
        .take(8)
        .cloned()
        .collect();
    widgets::section_count(ui, "HANDED DOWN", recent.len(), theme::DIM);

    if recent.is_empty() {
        widgets::fitted_card(ui, widgets::Card::default(), |ui| {
            super::nothing_yet(
                ui,
                "NOTHING HANDED DOWN YET",
                "No delegation has reached this panel. Carl hands work down rather than doing it \
                 himself, so this fills as soon as he does.",
            );
        });
        return;
    }

    ScrollArea::vertical()
        .id_salt("carl-delegations")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for d in &recent {
                widgets::card(
                    ui,
                    68.0,
                    widgets::Card::default().tone(widgets::Tone::Quiet),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{} to {}", d.from, d.to))
                                    .font(theme::label())
                                    .color(theme::COLD),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(widgets::ago(now.max(d.at), d.at))
                                        .font(theme::label())
                                        .color(theme::FAINT),
                                );
                            });
                        });
                        ui.add(
                            Label::new(
                                RichText::new(&d.goal)
                                    .font(theme::prose())
                                    .color(theme::DIM),
                            )
                            .truncate(),
                        );
                    },
                );
            }
        });
}
