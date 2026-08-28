//! The task in hand, and what would prove it done.

use eframe::egui::{Align, Label, Layout, RichText, Ui, vec2};

use crate::app::App;
use crate::command::WorkspaceRequest;
use crate::model::AgentView;
use crate::theme;
use crate::ui::widgets::{self, Mark};

pub fn draw(app: &mut App, ui: &mut Ui, view: &AgentView) {
    let task = view
        .task
        .as_ref()
        .and_then(|id| app.snapshot.task(id))
        .or_else(|| app.snapshot.tasks.iter().find(|t| t.owner == view.name))
        .cloned();

    let Some(task) = task else {
        widgets::card(ui, 86.0, widgets::Card::default(), |ui| {
            widgets::state_chip(ui, Mark::Hollow, "NO TASK IN HAND", theme::UNKNOWN);
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "The backend has not put a task with this agent. That is a fact about the \
                     record rather than a guess from what it last did.",
                )
                .font(theme::prose())
                .color(theme::DIM),
            );
        });
        return;
    };

    let must = task.must.len().max(1) as f32;
    let mut diff: Option<String> = None;

    widgets::card(
        ui,
        188.0 + must * 22.0,
        widgets::Card::default().attention(view.blocker.is_some()),
        |ui| {
            ui.add(Label::new(
                RichText::new(&task.goal)
                    .font(theme::prose())
                    .color(theme::TEXT),
            ));
            ui.add_space(8.0);
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
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(11.0, 11.0), eframe::egui::Sense::hover());
                    widgets::mark(ui.painter(), rect, Mark::Barred, theme::BAD);
                    ui.add_space(5.0);
                    ui.add(Label::new(
                        RichText::new(blocker)
                            .font(theme::prose())
                            .color(theme::BAD),
                    ));
                });
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                widgets::small(ui, "DONE WHEN");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if crate::ui::shell::open_link(ui, "see what changed") {
                        diff = Some(task.id.clone());
                    }
                });
            });
            for item in &task.must {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(9.0, 9.0), eframe::egui::Sense::hover());
                    widgets::mark(ui.painter(), rect, Mark::Hollow, theme::COLD);
                    ui.add_space(5.0);
                    ui.add(
                        Label::new(RichText::new(item).font(theme::prose()).color(theme::DIM))
                            .truncate(),
                    );
                });
            }
        },
    );

    if let Some(task) = diff {
        app.open_workspace(WorkspaceRequest::Diff { task });
    }
}
