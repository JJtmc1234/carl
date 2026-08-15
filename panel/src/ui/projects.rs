//! Work, with milestones weighted above the chatter under them.
//!
//! A project list is easy to turn into a task feed, and a task feed is the one thing that makes
//! it useless: forty rows that all look the same say nothing about whether anything is moving.
//! So the milestones are the largest text on the row, the phase is a rail rather than a
//! percentage, and individual tasks are a count rather than a list.

use eframe::egui::{Align, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::model::Phase;
use crate::theme;

use super::{shell, widgets};

/// How many milestones a project shows before it becomes a history.
pub const MILESTONES_SHOWN: usize = 4;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.34);
    ui.horizontal_top(|ui| {
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| list(app, ui));
        });
        ui.add_space(16.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| detail(app, ui));
        });
    });
}

fn list(app: &mut App, ui: &mut Ui) {
    widgets::section(ui, "PROJECTS");
    let projects = app.snapshot.projects.clone();

    for p in projects {
        let selected = app.project.as_deref() == Some(p.name.as_str());
        let response = widgets::row(ui, 46.0, selected, false, |ui| {
            ui.horizontal(|ui| {
                widgets::pip(
                    ui,
                    if p.blocked() {
                        theme::BAD
                    } else {
                        phase_color(p.phase)
                    },
                    p.phase != Phase::Unknown,
                );
                ui.label(
                    RichText::new(&p.name)
                        .font(theme::body())
                        .color(if selected { theme::ACCENT } else { theme::TEXT }),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(p.phase.label())
                            .font(theme::label())
                            .color(phase_color(p.phase)),
                    );
                });
            });
            ui.label(
                RichText::new(p.status.clone().unwrap_or_else(|| "no status given".into()))
                    .font(theme::label())
                    .color(if p.status.is_some() {
                        theme::DIM
                    } else {
                        theme::UNKNOWN
                    }),
            );
        });
        if response.clicked() {
            app.select_project(&p.name);
        }
    }
}

fn detail(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.project.clone() else {
        ui.add_space(40.0);
        ui.label(
            RichText::new("select a project")
                .font(theme::label())
                .color(theme::FAINT),
        );
        return;
    };
    let Some(p) = app.snapshot.project(&name).cloned() else {
        return;
    };
    let now = app.snapshot.at;

    ScrollArea::vertical()
        .id_salt("project-detail")
        .show(ui, |ui| {
            ui.label(
                RichText::new(theme::spaced(&p.name.to_uppercase()))
                    .font(theme::big())
                    .color(theme::TEXT),
            );
            ui.label(RichText::new(&p.goal).font(theme::body()).color(theme::DIM));
            ui.add_space(12.0);

            rail(ui, p.phase);
            ui.add_space(12.0);

            widgets::field(ui, "owner", p.owner.as_deref());
            let agents = p.active_agents.join(", ");
            widgets::field(
                ui,
                "active agents",
                if agents.is_empty() {
                    None
                } else {
                    Some(agents.as_str())
                },
            );
            widgets::field(ui, "active tasks", Some(&p.active_tasks.len().to_string()));
            widgets::field(ui, "next objective", p.next_objective.as_deref());

            if !p.blockers.is_empty() {
                ui.add_space(10.0);
                widgets::section(ui, "BLOCKED BY");
                for b in &p.blockers {
                    ui.label(RichText::new(b).font(theme::label()).color(theme::BAD));
                }
            }

            ui.add_space(14.0);
            widgets::section(ui, "MILESTONES");
            let recent = p.recent_milestones(MILESTONES_SHOWN);
            if recent.is_empty() {
                ui.label(
                    RichText::new("nothing recorded as a milestone yet")
                        .font(theme::label())
                        .color(theme::UNKNOWN),
                );
            }
            for m in recent {
                ui.horizontal(|ui| {
                    widgets::pip(ui, theme::ACCENT, true);
                    // The largest text on this pane, because a milestone outranks everything under it.
                    ui.label(
                        RichText::new(&m.title)
                            .font(theme::body())
                            .color(theme::TEXT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(widgets::ago(now, m.at))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    });
                });
                if let Some(detail) = &m.detail {
                    ui.horizontal(|ui| {
                        ui.add_space(15.0);
                        ui.label(RichText::new(detail).font(theme::label()).color(theme::DIM));
                    });
                }
                ui.add_space(8.0);
            }
        });
}

/// The phase rail. Draws no position at all when nobody has said what the phase is, rather
/// than defaulting to the beginning.
fn rail(ui: &mut Ui, phase: Phase) {
    let at = phase.step();
    ui.horizontal(|ui| {
        for (n, name) in Phase::STEPS.iter().enumerate() {
            let reached = at.is_some_and(|a| n <= a);
            let here = at == Some(n);
            ui.label(
                RichText::new(theme::spaced(name))
                    .font(theme::label())
                    .color(match (reached, here) {
                        (_, true) => theme::ACCENT,
                        (true, false) => theme::DIM,
                        _ => theme::FAINT,
                    }),
            );
            if n + 1 < Phase::STEPS.len() {
                ui.label(
                    RichText::new("-")
                        .font(theme::label())
                        .color(theme::RULE_BRIGHT),
                );
            }
        }
    });
    if at.is_none() {
        ui.label(
            RichText::new(format!("phase {}", phase.label().to_lowercase()))
                .font(theme::label())
                .color(theme::UNKNOWN),
        );
    }
}

fn phase_color(p: Phase) -> eframe::egui::Color32 {
    match p {
        Phase::Building => theme::ACCENT,
        Phase::Verifying => theme::COLD,
        Phase::Done => theme::GOOD,
        Phase::Paused => theme::WARN,
        Phase::Planned => theme::FAINT,
        Phase::Unknown => theme::UNKNOWN,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Project;

    /// A milestone list is capped and newest first, or the front of the panel becomes a history
    /// nobody reads.
    #[test]
    fn milestones_are_capped_at_what_the_pane_shows() {
        let mut p = Project::new("x", "y");
        for n in 0..12 {
            p.milestones.push(crate::model::Milestone {
                at: n * 10,
                title: format!("m{n}"),
                detail: None,
            });
        }
        let shown = p.recent_milestones(MILESTONES_SHOWN);
        assert_eq!(shown.len(), MILESTONES_SHOWN);
        assert_eq!(shown[0].title, "m11", "newest first");
    }
}
