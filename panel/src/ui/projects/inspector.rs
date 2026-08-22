//! One project in full: who owns it, what is holding it, what is being worked on, and what
//! has actually been proven.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::widgets::{self, Mark};

use super::{MILESTONES_SHOWN, status_color, status_label, status_mark};

pub fn draw(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.project.clone() else {
        waiting(ui);
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
        .auto_shrink([false, false])
        .show(ui, |ui| {
            widgets::card(ui, 168.0, widgets::Card::default(), |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::spaced(&p.project.name.to_uppercase())
                            .font(theme::title())
                            .color(theme::TEXT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        widgets::state_chip(
                            ui,
                            status_mark(p.project.status),
                            status_label(p.project.status),
                            status_color(p.project.status),
                        );
                    });
                });
                ui.add(Label::new(
                    RichText::new(&p.project.goal)
                        .font(theme::prose())
                        .color(theme::DIM),
                ));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&p.project.phase)
                        .font(theme::prose())
                        .color(theme::ACCENT),
                );
            });
            ui.add_space(theme::GAP);

            if !p.project.blockers.is_empty() {
                widgets::card(
                    ui,
                    58.0 + 26.0 * p.project.blockers.len() as f32,
                    widgets::Card::default().attention(true),
                    |ui| {
                        widgets::state_chip(ui, Mark::Barred, "HELD UP BY", theme::BAD);
                        ui.add_space(4.0);
                        for b in &p.project.blockers {
                            ui.add(Label::new(
                                RichText::new(b).font(theme::prose()).color(theme::TEXT),
                            ));
                        }
                    },
                );
                ui.add_space(theme::GAP);
            }

            widgets::section(ui, "WHO OWNS IT");
            widgets::field(ui, "department", p.project.department.as_deref());
            let agents = p.active_agents.join(", ");
            widgets::field(
                ui,
                "agents on it",
                if agents.is_empty() {
                    None
                } else {
                    Some(agents.as_str())
                },
            );
            widgets::field(ui, "next objective", p.project.next_objective.as_deref());
            widgets::field(ui, "path", p.project.path.as_ref().and_then(|x| x.to_str()));
            ui.add_space(theme::GAP);

            widgets::section_count(ui, "ACTIVE WORK", tasks.len(), theme::DIM);
            if tasks.is_empty() {
                ui.label(
                    RichText::new(
                        "The record puts no task in this project. Tasks are linked by the \
                         backend, so this is what the record says rather than what the goals \
                         happen to read like.",
                    )
                    .font(theme::prose())
                    .color(theme::UNKNOWN),
                );
            }
            for task in &tasks {
                widgets::card(ui, 84.0, widgets::Card::default(), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&task.owner)
                                .font(theme::body())
                                .color(theme::TEXT),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(&task.status)
                                    .font(theme::label())
                                    .color(theme::COLD),
                            );
                        });
                    });
                    ui.add(
                        Label::new(
                            RichText::new(&task.goal)
                                .font(theme::prose())
                                .color(theme::DIM),
                        )
                        .truncate(),
                    );
                    // Walking project to task to agent, using the link the record carries
                    // rather than a second relationship kept in the interface.
                    if crate::ui::shell::open_link(ui, &format!("open {}", task.owner)) {
                        walk_to = Some(task.owner.clone());
                    }
                });
            }
            ui.add_space(theme::GAP);

            milestones(ui, &p, now);
        });

    if let Some(agent) = walk_to {
        app.select_agent(&agent);
        app.select_tab(crate::app::Tab::Agents);
    }
}

fn milestones(ui: &mut Ui, p: &crate::model::ProjectView, now: u64) {
    widgets::section_count(ui, "MILESTONES", p.milestones.len(), theme::DIM);

    // A milestone file with lines that would not parse is a hole in the history, and the pane
    // says so. Skipping them quietly is right, because one bad append must not hide every
    // milestone after it, but it also means a reader would never learn the list is short
    // unless it is said out loud.
    if p.milestone_gaps > 0 {
        widgets::state_chip(
            ui,
            Mark::Half,
            &format!(
                "{} LINE(S) UNREADABLE, THIS LIST IS INCOMPLETE",
                p.milestone_gaps
            ),
            theme::WARN,
        );
        ui.add_space(6.0);
    }
    if p.milestones.is_empty() {
        ui.label(
            RichText::new("Nothing has been recorded as a milestone yet.")
                .font(theme::prose())
                .color(theme::UNKNOWN),
        );
        return;
    }

    // Newest first is the order the provider keeps, so nothing is re sorted here.
    for m in p.milestones.iter().take(MILESTONES_SHOWN) {
        let extras = m.detail.is_some() as u8 + m.evidence.is_some() as u8;
        widgets::card(
            ui,
            56.0 + 24.0 * extras as f32,
            widgets::Card::default(),
            |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(10.0, 10.0), eframe::egui::Sense::hover());
                    widgets::mark(ui.painter(), rect, Mark::Filled, theme::GOOD);
                    ui.add_space(5.0);
                    ui.add(
                        Label::new(
                            RichText::new(&m.title)
                                .font(theme::body())
                                .color(theme::TEXT),
                        )
                        .truncate(),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(widgets::ago(now.max(m.at), m.at))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    });
                });
                if let Some(detail) = &m.detail {
                    ui.add(
                        Label::new(RichText::new(detail).font(theme::prose()).color(theme::DIM))
                            .truncate(),
                    );
                }
                if let Some(evidence) = &m.evidence {
                    ui.add(
                        Label::new(
                            RichText::new(format!("evidence  {evidence}"))
                                .font(theme::label())
                                .color(theme::COLD),
                        )
                        .truncate(),
                    );
                }
            },
        );
    }
}

/// Nothing picked. Says what this pane is for rather than sitting blank.
fn waiting(ui: &mut Ui) {
    widgets::card(ui, 132.0, widgets::Card::default(), |ui| {
        widgets::state_chip(ui, Mark::Hollow, "NO PROJECT SELECTED", theme::FAINT);
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Pick one on the left to see who owns it, what is holding it up, the tasks the \
                 record puts inside it, and the milestones with the evidence for each.",
            )
            .font(theme::prose())
            .color(theme::DIM),
        );
    });
}
