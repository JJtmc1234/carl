//! One agent in full, and the one place JJ can reach past the chain.
//!
//! The intervention block is drawn in a colour used nowhere else in the interface and is
//! separated from everything above it by a labelled rule. That is not decoration. Going around
//! the chain is the act the whole army is arranged to avoid, so it must be impossible to do
//! while believing you were sending an ordinary message.

use eframe::egui::{Align, Layout, RichText, ScrollArea, TextEdit, Ui, vec2};

use crate::app::App;
use crate::command::{InterventionKind, WorkspaceRequest};
use crate::model::ProcessState;
use crate::theme;

use super::widgets;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.agent.clone() else {
        ui.add_space(40.0);
        ui.label(
            RichText::new("select an agent")
                .font(theme::label())
                .color(theme::FAINT),
        );
        return;
    };
    let Some(view) = app.snapshot.agent(&name).cloned() else {
        return;
    };
    let now = app.snapshot.at;

    ScrollArea::vertical()
        .id_salt("agent-detail")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(theme::spaced(&view.name.to_uppercase()))
                        .font(theme::big())
                        .color(theme::TEXT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(view.status.label())
                            .font(theme::label())
                            .color(widgets::status_color(view.status)),
                    );
                });
            });

            if let Some(agent) = view.agent() {
                ui.label(
                    RichText::new(agent.remit)
                        .font(theme::label())
                        .color(theme::DIM),
                );
            }
            ui.add_space(12.0);

            widgets::section(ui, "REPORTING LINE");
            let chain = carl::army::org::chain_to_root(&view.name);
            let line = chain
                .iter()
                .map(|a| a.name)
                .collect::<Vec<_>>()
                .join("  <  ");
            ui.label(RichText::new(line).font(theme::body()).color(theme::COLD));
            ui.add_space(4.0);
            widgets::field(ui, "rank", view.rank().map(|r| r.to_string()).as_deref());
            widgets::field(ui, "department", view.department.as_deref());
            widgets::field(ui, "sub department", view.sub_department.as_deref());
            ui.add_space(12.0);

            widgets::section(ui, "CURRENT TASK");
            task_block(app, ui, &view);
            ui.add_space(12.0);

            widgets::section(ui, "PROCESS");
            widgets::field(ui, "model", view.model.as_deref());
            widgets::field(
                ui,
                "process",
                view.process.map(|p| match p {
                    ProcessState::Running => "running",
                    ProcessState::Stopped => "stopped",
                }),
            );
            widgets::field(ui, "worktree", view.worktree.as_deref());
            widgets::field(ui, "branch", view.branch.as_deref());
            if let Some(cwd) = view.worktree.clone()
                && super::shell::open_link(ui, "open a shell here")
            {
                app.open_workspace(WorkspaceRequest::Terminal { cwd });
            }
            ui.add_space(12.0);

            widgets::section(ui, "RECENT EVENTS");
            let events: Vec<_> = app
                .snapshot
                .events
                .iter()
                .filter(|r| r.actor == view.name)
                .rev()
                .take(6)
                .cloned()
                .collect();
            if events.is_empty() {
                ui.label(
                    RichText::new("nothing recorded for this agent")
                        .font(theme::label())
                        .color(theme::UNKNOWN),
                );
            }
            for record in events {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(record.event.kind())
                            .font(theme::label())
                            .color(theme::COLD),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(widgets::ago(now, record.at))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    });
                });
            }

            ui.add_space(20.0);
            intervention(app, ui, &view.name);
        });
}

fn task_block(app: &mut App, ui: &mut Ui, view: &crate::model::AgentView) {
    let task = view
        .task
        .as_ref()
        .and_then(|id| app.snapshot.task(id))
        .or_else(|| app.snapshot.tasks.iter().find(|t| t.owner == view.name))
        .cloned();

    let Some(task) = task else {
        ui.label(
            RichText::new("no task in hand")
                .font(theme::label())
                .color(theme::UNKNOWN),
        );
        return;
    };

    ui.label(
        RichText::new(&task.goal)
            .font(theme::body())
            .color(theme::TEXT),
    );
    ui.add_space(6.0);
    widgets::field(ui, "status", Some(&task.status));
    widgets::field(ui, "assigned by", Some(&task.assigner));
    widgets::field(
        ui,
        "attempts",
        Some(&format!(
            "{} of {}",
            task.attempts,
            carl::army::MAX_ATTEMPTS
        )),
    );
    if let Some(blocker) = &view.blocker {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("blocked. {blocker}"))
                .font(theme::label())
                .color(theme::BAD),
        );
    }

    ui.add_space(6.0);
    widgets::small(ui, "DONE WHEN");
    for must in &task.must {
        ui.label(
            RichText::new(format!("  {must}"))
                .font(theme::label())
                .color(theme::DIM),
        );
    }

    ui.add_space(6.0);
    let id = task.id.clone();
    if super::shell::open_link(ui, "see what changed") {
        app.open_workspace(WorkspaceRequest::Diff { task: id });
    }
}

/// The one place JJ goes around the chain.
fn intervention(app: &mut App, ui: &mut Ui, agent: &str) {
    eframe::egui::Frame::none()
        .fill(theme::PANEL)
        .stroke(theme::edge(theme::INTERVENE))
        .rounding(theme::CORNER)
        .inner_margin(eframe::egui::Margin::symmetric(12.0, 10.0))
        .show(ui, |ui| {
            ui.label(
                RichText::new(theme::spaced("DIRECT JJ INTERVENTION"))
                    .font(theme::label())
                    .color(theme::INTERVENE),
            );
            ui.label(
                RichText::new(format!(
                    "Goes straight to {agent} and around {}. Recorded as JJ acting directly.",
                    carl::army::org::find(agent)
                        .and_then(|a| a.reports_to)
                        .unwrap_or("the chain")
                ))
                .font(theme::label())
                .color(theme::DIM),
            );
            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                for kind in InterventionKind::ALL {
                    let active = app
                        .intervening
                        .as_ref()
                        .is_some_and(|i| i.kind == kind && i.agent == agent);
                    let text = RichText::new(kind.label())
                        .font(theme::label())
                        .color(if active { theme::INTERVENE } else { theme::DIM });
                    if ui.button(text).clicked() {
                        app.select_agent(agent);
                        app.begin_intervention(kind);
                    }
                }
            });

            let composing = app
                .intervening
                .as_ref()
                .filter(|i| i.agent == agent)
                .cloned();
            let Some(current) = composing else {
                return;
            };

            ui.add_space(8.0);
            widgets::small(ui, &current.kind.prompt().to_uppercase());
            if let Some(i) = app.intervening.as_mut() {
                ui.add_sized(
                    vec2(ui.available_width(), 58.0),
                    TextEdit::multiline(&mut i.body)
                        .font(theme::body())
                        .hint_text(current.kind.prompt()),
                );
            }

            if let Some(warning) = app.intervention_warning() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(warning)
                        .font(theme::label())
                        .color(theme::INTERVENE),
                );
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let confirming = app.intervening.as_ref().is_some_and(|i| i.confirming);
                let label = if confirming {
                    "CONFIRM AND SEND"
                } else if current.kind.is_forceful() {
                    "REVIEW"
                } else {
                    "SEND"
                };
                let ready = app.intervention_ready();
                if ui
                    .add_enabled(
                        ready,
                        eframe::egui::Button::new(
                            RichText::new(label)
                                .font(theme::label())
                                .color(theme::INTERVENE),
                        ),
                    )
                    .clicked()
                {
                    app.send_intervention();
                }
                if ui
                    .button(RichText::new("CANCEL").font(theme::label()))
                    .clicked()
                {
                    app.cancel_intervention();
                }
            });
        });
}
