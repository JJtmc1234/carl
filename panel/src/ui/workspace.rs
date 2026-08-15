//! The contextual workspace: a container, and nothing that fills it.
//!
//! Docked at the bottom rather than given a tab of its own, because a terminal or an editor is
//! a tool you open from something you were already looking at. Making it a fifth destination
//! would invert that, and you would go to the editor and then hunt for the file.
//!
//! Everything below is the visual container and the interaction model. Process 3 owns opening
//! files, running shells and producing diffs. This branch spawns nothing and reads nothing,
//! and where content will go it says so rather than showing a convincing fake.

use eframe::egui::{Align, Context, Layout, RichText, TopBottomPanel};

use crate::app::App;
use crate::command::WorkspaceRequest;
use crate::theme;

/// How tall the pane sits by default. Enough for a shell to be useful, little enough that the
/// tab above it is still the thing you are working in.
pub const HEIGHT: f32 = 240.0;

pub fn draw(app: &mut App, ctx: &Context) {
    let Some(workspace) = app.workspace.clone() else {
        return;
    };

    TopBottomPanel::bottom("workspace")
        .exact_height(HEIGHT)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::PANEL)
                .stroke(theme::hairline())
                .inner_margin(eframe::egui::Margin::symmetric(12.0, 10.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(theme::spaced("WORKSPACE"))
                        .font(theme::label())
                        .color(theme::DIM),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(workspace.open.title())
                        .font(theme::body())
                        .color(theme::ACCENT),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .button(RichText::new("CLOSE").font(theme::label()))
                        .clicked()
                    {
                        app.close_workspace();
                    }
                });
            });
            ui.add_space(8.0);
            super::widgets::rule(ui);
            ui.add_space(10.0);

            match &workspace.content {
                Some(text) => {
                    ui.label(RichText::new(text).font(theme::body()).color(theme::TEXT));
                }
                None => waiting(ui, &workspace.open),
            }
        });
}

/// What the pane says while nothing can fill it yet.
///
/// It names the exact request it would hand over, which is the integration point written on
/// the screen rather than only in a document.
fn waiting(ui: &mut eframe::egui::Ui, request: &WorkspaceRequest) {
    let (what, target) = match request {
        WorkspaceRequest::File { path, line } => (
            "editor",
            match line {
                Some(l) => format!("{path} at line {l}"),
                None => path.clone(),
            },
        ),
        WorkspaceRequest::Diff { task } => ("diff viewer", format!("task {task}")),
        WorkspaceRequest::Terminal { cwd } => ("terminal", cwd.clone()),
        WorkspaceRequest::Investigate { component } => ("investigation", component.clone()),
        WorkspaceRequest::Close => ("nothing", String::new()),
    };

    ui.label(
        RichText::new(format!("{} not attached yet", what))
            .font(theme::body())
            .color(theme::DIM),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(target)
            .font(theme::label())
            .color(theme::COLD),
    );
    ui.add_space(10.0);
    ui.label(
        RichText::new(
            "The panel owns this container. Whatever fills it is handed over as a \
             WorkspaceRequest, and nothing in this build opens a file or starts a shell.",
        )
        .font(theme::label())
        .color(theme::FAINT),
    );
}
