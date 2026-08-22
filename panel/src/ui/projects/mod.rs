//! Work, with the things that say whether it is moving weighted above the chatter under them.
//!
//! A project list is easy to turn into a task feed, and a task feed is the thing that makes it
//! useless: forty rows that all look the same say nothing about whether anything is moving. So
//! the phase is the project's own word, milestones are the evidence, and the tasks are a list
//! you can walk from a project to the agent holding it.
//!
//! **No percentage anywhere.** Nothing in the record knows what fraction of a project is done.
//! A progress bar here would be a number the panel made up, and a made up number on a screen
//! like this is worse than a gap, because a gap is obviously a gap.
//!
//! Nothing here infers. A project shows the tasks the record says are its own, through
//! `TaskView::project`, and shows none when the record says none.

use eframe::egui::{Align, Color32, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::model::Status;
use crate::theme;

use super::{shell, widgets};

mod inspector;

/// How many milestones a project shows before it becomes a history.
pub const MILESTONES_SHOWN: usize = 4;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.34);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| list(app, ui));
        });
        ui.add_space(theme::GAP + 4.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| inspector::draw(app, ui));
        });
    });
}

fn list(app: &mut App, ui: &mut Ui) {
    let projects: Vec<crate::model::ProjectView> =
        super::overview::project_order(&app.snapshot.projects)
            .into_iter()
            .cloned()
            .collect();
    widgets::section_count(ui, "PROJECTS", projects.len(), theme::DIM);

    if projects.is_empty() {
        empty(ui);
        return;
    }

    let mut pick = None;
    ScrollArea::vertical()
        .id_salt("project-list")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for p in &projects {
                let selected = app.project.as_deref() == Some(p.project.name.as_str());
                let blocked = !p.project.blockers.is_empty();
                let response = widgets::card(
                    ui,
                    if blocked { 122.0 } else { 100.0 },
                    widgets::Card::default()
                        .selected(selected)
                        .attention(blocked),
                    |ui| {
                        ui.horizontal(|ui| {
                            let (rect, _) = ui.allocate_exact_size(
                                vec2(11.0, 11.0),
                                eframe::egui::Sense::hover(),
                            );
                            widgets::mark(
                                ui.painter(),
                                rect,
                                status_mark(p.project.status),
                                status_color(p.project.status),
                            );
                            ui.add_space(5.0);
                            ui.add(
                                Label::new(
                                    RichText::new(&p.project.name)
                                        .font(theme::heading())
                                        .color(if selected { theme::ACCENT } else { theme::TEXT }),
                                )
                                .truncate(),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(status_label(p.project.status))
                                        .font(theme::label())
                                        .color(status_color(p.project.status)),
                                );
                            });
                        });
                        ui.add(
                            Label::new(
                                RichText::new(&p.project.phase)
                                    .font(theme::prose())
                                    .color(theme::DIM),
                            )
                            .truncate(),
                        );
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            track(ui, p.milestones.len());
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(
                                        p.project
                                            .department
                                            .clone()
                                            .unwrap_or_else(|| "no department".into()),
                                    )
                                    .font(theme::label())
                                    .color(
                                        if p.project.department.is_some() {
                                            theme::FAINT
                                        } else {
                                            theme::UNKNOWN
                                        },
                                    ),
                                );
                            });
                        });
                        if blocked {
                            ui.add(
                                Label::new(
                                    RichText::new(p.project.blockers.join(". "))
                                        .font(theme::prose())
                                        .color(theme::BAD),
                                )
                                .truncate(),
                            );
                        }
                    },
                );
                if response.clicked() {
                    pick = Some(p.project.name.clone());
                }
            }
        });

    if let Some(name) = pick {
        app.select_project(&name);
    }
}

/// Milestones reached, drawn as marks rather than as a bar.
///
/// A bar needs a denominator and there is not one. Nobody has said how many milestones a
/// project has, so this counts what has happened and says so, which is a fact, where a bar
/// would be a claim about how much is left.
fn track(ui: &mut Ui, reached: usize) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for _ in 0..reached.min(8) {
            let (rect, _) = ui.allocate_exact_size(vec2(9.0, 9.0), eframe::egui::Sense::hover());
            widgets::mark(ui.painter(), rect, widgets::Mark::Filled, theme::GOOD);
        }
        ui.add_space(4.0);
        ui.label(
            RichText::new(match reached {
                0 => "no milestone recorded".to_string(),
                1 => "1 milestone".to_string(),
                n => format!("{n} milestones"),
            })
            .font(theme::label())
            .color(if reached == 0 {
                theme::UNKNOWN
            } else {
                theme::DIM
            }),
        );
    });
}

/// The empty state, which is a real one rather than a heading over a blank pane.
fn empty(ui: &mut Ui) {
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
        widgets::state_chip(
            ui,
            widgets::Mark::Dash,
            "THE BACKEND IS CARRYING NO PROJECTS",
            theme::UNKNOWN,
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(
                "This is a gap in the record rather than a quiet week. A project appears here \
                 once one is registered with the backend, and everything on this screen is read \
                 from that registration rather than guessed at from what agents are doing.",
            )
            .font(theme::prose())
            .color(theme::DIM),
        );
        ui.add_space(12.0);
        widgets::small(ui, "WHAT WOULD SHOW UP HERE");
        for line in [
            "the phase, in the project's own words",
            "anything holding it up, and who owns it",
            "the tasks the record puts inside it",
            "milestones, with the evidence for each",
        ] {
            ui.label(
                RichText::new(format!("  {line}"))
                    .font(theme::prose())
                    .color(theme::FAINT),
            );
        }
    });
}

pub fn status_label(s: Status) -> &'static str {
    match s {
        Status::Active => "ACTIVE",
        Status::Paused => "PAUSED",
        Status::Done => "DONE",
        Status::Abandoned => "ABANDONED",
    }
}

pub fn status_color(s: Status) -> Color32 {
    match s {
        Status::Active => theme::ACCENT,
        Status::Paused => theme::WARN,
        Status::Done => theme::GOOD,
        Status::Abandoned => theme::UNKNOWN,
    }
}

/// A status is never carried by its colour alone here either.
pub fn status_mark(s: Status) -> widgets::Mark {
    match s {
        Status::Active => widgets::Mark::Filled,
        Status::Paused => widgets::Mark::Barred,
        Status::Done => widgets::Mark::Half,
        Status::Abandoned => widgets::Mark::Dash,
    }
}
