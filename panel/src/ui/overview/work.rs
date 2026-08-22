//! Which projects are moving, on the front page.
//!
//! Active first, because a paused project is not news. Each card carries the phase in the
//! project's own words rather than a percentage, since nothing in the record knows what
//! fraction of a project is done and putting a bar there would be inventing one.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::{App, Tab};
use crate::model::{ProjectView, Status};
use crate::theme;
use crate::ui::widgets::{self, Mark};

/// Active first, then anything else, keeping the order the backend sent inside each group.
pub fn ordered(projects: &[ProjectView]) -> Vec<&ProjectView> {
    let mut out: Vec<&ProjectView> = projects.iter().collect();
    out.sort_by_key(|p| match p.project.status {
        Status::Active if !p.project.blockers.is_empty() => 0,
        Status::Active => 1,
        Status::Paused => 2,
        Status::Done => 3,
        Status::Abandoned => 4,
    });
    out
}

pub fn draw(app: &mut App, ui: &mut Ui) {
    let projects: Vec<ProjectView> = ordered(&app.snapshot.projects)
        .into_iter()
        .cloned()
        .collect();
    let active = projects
        .iter()
        .filter(|p| p.project.status == Status::Active)
        .count();
    widgets::section_count(ui, "PROJECTS ACTIVE", active, theme::DIM);

    if projects.is_empty() {
        widgets::fitted_card(ui, widgets::Card::default(), |ui| {
            widgets::state_chip(ui, Mark::Dash, "NO PROJECTS ON THE BACKEND", theme::UNKNOWN);
            ui.add_space(6.0);
            ui.label(
                RichText::new("Nothing has been registered as a project yet.")
                    .font(theme::prose())
                    .color(theme::DIM),
            );
        });
        return;
    }

    let mut pick: Option<String> = None;
    ScrollArea::vertical()
        .id_salt("overview-projects")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for p in &projects {
                let blocked = !p.project.blockers.is_empty();
                let response = widgets::card(
                    ui,
                    if blocked { 106.0 } else { 84.0 },
                    widgets::Card::default()
                        .attention(blocked)
                        .selected(app.project.as_deref() == Some(p.project.name.as_str())),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&p.project.name)
                                    .font(theme::body())
                                    .color(theme::TEXT),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(super::super::projects::status_label(
                                        p.project.status,
                                    ))
                                    .font(theme::label())
                                    .color(super::super::projects::status_color(p.project.status)),
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
                        if blocked {
                            ui.allocate_ui(vec2(ui.available_width(), 26.0), |ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(p.project.blockers.join(". "))
                                            .font(theme::prose())
                                            .color(theme::BAD),
                                    )
                                    .truncate(),
                                );
                            });
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
        app.select_tab(Tab::Projects);
    }
}

#[cfg(test)]
mod tests;
