//! Work, with milestones weighted above the chatter under them.
//!
//! A project list is easy to turn into a task feed, and a task feed is the thing that makes it
//! useless: forty rows that all look the same say nothing about whether anything is moving. So
//! milestones are the largest text on the pane, the phase is the project's own word rather than
//! a percentage, and tasks are a list you can walk to an agent from.
//!
//! Nothing here infers. A project shows the tasks the record says are its own, through
//! `TaskView::project`, and shows none when the record says none.

use eframe::egui::{Align, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::model::Status;
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

    if projects.is_empty() {
        ui.label(
            RichText::new("the backend is carrying no projects")
                .font(theme::label())
                .color(theme::UNKNOWN),
        );
    }

    for p in projects {
        let selected = app.project.as_deref() == Some(p.project.name.as_str());
        let blocked = !p.project.blockers.is_empty();
        let response = widgets::row(ui, 46.0, selected, false, |ui| {
            ui.horizontal(|ui| {
                widgets::pip(
                    ui,
                    if blocked {
                        theme::BAD
                    } else {
                        status_color(p.project.status)
                    },
                    true,
                );
                ui.label(
                    RichText::new(&p.project.name)
                        .font(theme::body())
                        .color(if selected { theme::ACCENT } else { theme::TEXT }),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(status_label(p.project.status))
                            .font(theme::label())
                            .color(status_color(p.project.status)),
                    );
                });
            });
            ui.label(
                RichText::new(&p.project.phase)
                    .font(theme::label())
                    .color(theme::DIM),
            );
        });
        if response.clicked() {
            app.select_project(&p.project.name);
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
    let tasks: Vec<_> = app
        .snapshot
        .tasks_in(&p.project.id)
        .into_iter()
        .cloned()
        .collect();

    let mut walk_to: Option<String> = None;

    ScrollArea::vertical()
        .id_salt("project-detail")
        .show(ui, |ui| {
            ui.label(
                theme::spaced(&p.project.name.to_uppercase())
                    .font(theme::big())
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new(&p.project.goal)
                    .font(theme::body())
                    .color(theme::DIM),
            );
            ui.add_space(12.0);

            widgets::field(ui, "status", Some(status_label(p.project.status)));
            widgets::field(ui, "phase", Some(&p.project.phase));
            widgets::field(ui, "department", p.project.department.as_deref());
            widgets::field(ui, "next objective", p.project.next_objective.as_deref());
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
            widgets::field(ui, "path", p.project.path.as_ref().and_then(|x| x.to_str()));

            if !p.project.blockers.is_empty() {
                ui.add_space(10.0);
                widgets::section(ui, "BLOCKED BY");
                for b in &p.project.blockers {
                    ui.label(RichText::new(b).font(theme::label()).color(theme::BAD));
                }
            }

            ui.add_space(14.0);
            widgets::section(ui, "ACTIVE WORK");
            if tasks.is_empty() {
                ui.label(
                    RichText::new("the record puts no task in this project")
                        .font(theme::label())
                        .color(theme::UNKNOWN),
                );
            }
            for task in &tasks {
                // Walking project to task to agent, using the link the record carries rather
                // than a second relationship kept in the interface.
                if super::shell::open_link(ui, &format!("{}  {}", task.owner, task.goal)) {
                    walk_to = Some(task.owner.clone());
                }
            }

            ui.add_space(14.0);
            widgets::section(ui, "MILESTONES");
            // A milestone file with lines that would not parse is a hole in the history, and
            // the pane says so. Skipping them quietly is right, because one bad append must
            // not hide every milestone after it, but it also means a reader would never learn
            // the list is short unless it is said out loud.
            if p.milestone_gaps > 0 {
                ui.label(
                    RichText::new(format!(
                        "{} milestone line(s) could not be read, so this list is incomplete",
                        p.milestone_gaps
                    ))
                    .font(theme::label())
                    .color(theme::WARN),
                );
                ui.add_space(4.0);
            }
            if p.milestones.is_empty() {
                ui.label(
                    RichText::new("nothing recorded as a milestone yet")
                        .font(theme::label())
                        .color(theme::UNKNOWN),
                );
            }
            // Newest first is the order the provider keeps, so nothing is re sorted here.
            for m in p.milestones.iter().take(MILESTONES_SHOWN) {
                ui.horizontal(|ui| {
                    widgets::pip(ui, theme::ACCENT, true);
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
                if let Some(evidence) = &m.evidence {
                    ui.horizontal(|ui| {
                        ui.add_space(15.0);
                        ui.label(
                            RichText::new(format!("evidence  {evidence}"))
                                .font(theme::label())
                                .color(theme::COLD),
                        );
                    });
                }
                ui.add_space(8.0);
            }
        });

    if let Some(agent) = walk_to {
        app.select_agent(&agent);
        app.select_tab(crate::app::Tab::Agents);
    }
}

pub fn status_label(s: Status) -> &'static str {
    match s {
        Status::Active => "ACTIVE",
        Status::Paused => "PAUSED",
        Status::Done => "DONE",
        Status::Abandoned => "ABANDONED",
    }
}

fn status_color(s: Status) -> eframe::egui::Color32 {
    match s {
        Status::Active => theme::ACCENT,
        Status::Paused => theme::WARN,
        Status::Done => theme::GOOD,
        Status::Abandoned => theme::UNKNOWN,
    }
}
