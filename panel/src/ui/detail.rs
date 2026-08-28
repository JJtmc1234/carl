//! One agent in full, and the one place JJ can reach past the chain.
//!
//! The contextual inspector. Everything the card on the left could not carry, in cards of its
//! own so the sections are separable rather than one long column of key and value pairs.
//!
//! The intervention block is drawn in a colour used nowhere else in the interface and is
//! separated from everything above it. That is not decoration. Going around the chain is the
//! act the whole army is arranged to avoid, so it must be impossible to do while believing you
//! were sending an ordinary message.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::command::WorkspaceRequest;
use crate::model::ProcessState;
use crate::theme;

use super::widgets::{self, Mark};

mod intervention;
mod task;

pub fn draw(app: &mut App, ui: &mut Ui) {
    let Some(name) = app.agent.clone() else {
        waiting(ui);
        return;
    };
    let Some(view) = app.snapshot.agent(&name).cloned() else {
        return;
    };
    let now = app.snapshot.at;

    ScrollArea::vertical()
        .id_salt("agent-detail")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            header(ui, &view);
            ui.add_space(theme::GAP);
            reporting(ui, &view);
            ui.add_space(theme::GAP);

            widgets::section(ui, "CURRENT TASK");
            task::draw(app, ui, &view);
            ui.add_space(theme::GAP);

            process(app, ui, &view);
            ui.add_space(theme::GAP);
            events(app, ui, &view, now);

            ui.add_space(theme::GAP + 8.0);
            intervention::draw(app, ui, &view.name);
        });
}

fn header(ui: &mut Ui, view: &crate::model::AgentView) {
    let remit = view.agent().map(|a| a.remit).unwrap_or(
        "This agent is not in the organisation table, so nothing is known about what it is for.",
    );
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
        ui.horizontal(|ui| {
            ui.label(
                theme::spaced(&view.name.to_uppercase())
                    .font(theme::title())
                    .color(theme::TEXT),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                widgets::state_chip(
                    ui,
                    widgets::status_mark(view.status),
                    view.status.label(),
                    widgets::status_color(view.status),
                );
            });
        });
        ui.add_space(2.0);
        ui.label(
            RichText::new(super::agents::role_of(view))
                .font(theme::label())
                .color(theme::FAINT),
        );
        ui.add_space(6.0);
        ui.add(Label::new(
            RichText::new(remit).font(theme::prose()).color(theme::DIM),
        ));
    });
}

fn reporting(ui: &mut Ui, view: &crate::model::AgentView) {
    widgets::section(ui, "REPORTING LINE");
    let chain = carl::army::org::chain_to_root(&view.name);
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
        ui.horizontal_wrapped(|ui| {
            // An arrow between the names rather than the words "answers to". The words ran
            // together into "noraanswers tomason" when the card zeroed its spacing, and even
            // spaced correctly they made a chain of five read as a sentence rather than a line
            // of command. The arrow is shorter, cannot run together, and points the way
            // authority actually flows: upward, from the worker to the person.
            ui.spacing_mut().item_spacing.x = 8.0;
            for (i, agent) in chain.iter().enumerate() {
                if i > 0 {
                    ui.label(
                        RichText::new("\u{2192}")
                            .font(theme::body())
                            .color(theme::FAINT),
                    );
                }
                let human = agent.rank == carl::army::org::Rank::Human;
                ui.label(
                    RichText::new(widgets::proper(agent.name))
                        .font(theme::body())
                        .color(if human { theme::INTERVENE } else { theme::COLD }),
                );
            }
        });
        if let Some(rank) = view.rank() {
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("rank {rank:?}").to_lowercase())
                    .font(theme::label())
                    .color(theme::FAINT),
            );
        }
    });
}

fn process(app: &mut App, ui: &mut Ui, view: &crate::model::AgentView) {
    widgets::section(ui, "PROCESS");
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
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
    });
    if let Some(cwd) = view.worktree.clone() {
        ui.add_space(6.0);
        if super::shell::open_link(ui, "open a shell here") {
            app.open_workspace(WorkspaceRequest::Terminal { cwd });
        }
    }
}

fn events(app: &App, ui: &mut Ui, view: &crate::model::AgentView, now: u64) {
    let records: Vec<_> = app
        .snapshot
        .events
        .iter()
        .filter(|r| r.actor == view.name)
        .rev()
        .take(6)
        .cloned()
        .collect();
    widgets::section_count(ui, "RECENT EVENTS", records.len(), theme::DIM);

    if records.is_empty() {
        widgets::fitted_card(ui, widgets::Card::default(), |ui| {
            widgets::state_chip(ui, Mark::Dash, "NOTHING RECORDED", theme::UNKNOWN);
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "No journal record naming this agent has reached the panel in this session.",
                )
                .font(theme::prose())
                .color(theme::DIM),
            );
        });
        return;
    }

    for record in &records {
        widgets::card(
            ui,
            58.0,
            widgets::Card::default().tone(widgets::Tone::Quiet),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(record.event.kind())
                            .font(theme::label())
                            .color(theme::COLD),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(widgets::ago(now.max(record.at), record.at))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    });
                });
            },
        );
    }
}

/// Nothing selected. Says what the pane is for and what picking somebody gets you.
fn waiting(ui: &mut Ui) {
    widgets::fitted_card(ui, widgets::Card::default(), |ui| {
        widgets::state_chip(ui, Mark::Hollow, "NO AGENT SELECTED", theme::FAINT);
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Pick a card on the left to see the reporting line in full, the task in hand and \
                 what proves it done, the process and worktree behind it, and everything the \
                 journal has recorded for that agent.",
            )
            .font(theme::prose())
            .color(theme::DIM),
        );
        ui.add_space(10.0);
        ui.allocate_ui(vec2(ui.available_width(), 40.0), |ui| {
            ui.label(
                RichText::new(
                    "It is also the one place JJ can reach past the chain, which is why that \
                     block is drawn in a colour used nowhere else.",
                )
                .font(theme::prose())
                .color(theme::FAINT),
            );
        });
    });
}
