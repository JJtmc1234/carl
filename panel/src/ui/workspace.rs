//! The contextual workspace: a terminal, an editor, a comparison, or a reading in full.
//!
//! Docked at the bottom rather than given a tab of its own, because a terminal or an editor is
//! a tool you open from something you were already looking at. Making it a destination inverts
//! that, and you would go to the editor and then hunt for the file.
//!
//! Everything drawn here reads state the facade produced. Nothing in this file opens a process,
//! reads a path or runs a comparison, and the pane says plainly when the facade refused.

use eframe::egui::{Align, Context, Key, Layout, RichText, ScrollArea, TextEdit, TopBottomPanel};

use crate::app::{App, Comparison, Pane};
use crate::theme;

use super::widgets;

/// How tall the pane sits. Enough for a shell to be useful, little enough that the tab above
/// is still the thing you are working in.
pub const HEIGHT: f32 = 330.0;

/// Roughly how many rows and columns that is, for telling the pty how big it is.
const ROWS: u16 = 15;
const COLS: u16 = 120;

pub fn draw(app: &mut App, ctx: &Context) {
    if app.workspace.is_none() {
        return;
    }

    // Never more than half the window. A fixed height is fine on a large screen and squeezes
    // the screen above it off a small one, and egui does not refuse: it overlaps them, so the
    // workspace header lands on top of whatever the tab drew there. The probe caught CLOSE
    // sitting on a card at 1280x800.
    let room = (ctx.screen_rect().height() * 0.5).max(180.0);
    TopBottomPanel::bottom("workspace")
        .exact_height(HEIGHT.min(room))
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::PANEL)
                .stroke(theme::edge(theme::RULE_BRIGHT))
                .inner_margin(eframe::egui::Margin::symmetric(16.0, 12.0)),
        )
        .show(ctx, |ui| {
            let (title, trouble) = {
                let w = app.workspace.as_ref().expect("just checked");
                (w.title(), w.trouble.clone())
            };

            ui.horizontal(|ui| {
                ui.label(
                    theme::spaced("WORKSPACE")
                        .font(theme::label())
                        .color(theme::DIM),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(title)
                        .font(theme::heading())
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
            ui.add_space(6.0);
            widgets::rule(ui);
            ui.add_space(8.0);

            // The facade could not do it. Said here, in the pane that was opened, rather than
            // by nothing happening.
            if let Some(why) = trouble {
                ui.label(RichText::new(why).font(theme::prose()).color(theme::BAD));
                return;
            }

            match app.workspace.as_ref().map(|w| w.pane.clone()) {
                Some(Pane::Terminal { .. }) => terminal(app, ui),
                Some(Pane::Editor { .. }) => editor(app, ui),
                Some(Pane::Diff(outcome)) => comparison(ui, &outcome),
                Some(Pane::Investigating(found)) => investigation(ui, &found),
                _ => {}
            }
        });
}

fn terminal(app: &mut App, ui: &mut eframe::egui::Ui) {
    app.terminal_resize(ROWS, COLS);

    let (output, cwd, alive) = match app.workspace.as_ref().map(|w| w.pane.clone()) {
        Some(Pane::Terminal {
            output, cwd, alive, ..
        }) => (output, cwd, alive),
        _ => return,
    };

    ui.horizontal(|ui| {
        widgets::pip(ui, if alive { theme::GOOD } else { theme::UNKNOWN }, alive);
        ui.label(
            RichText::new(cwd.unwrap_or_else(|| "no working directory".into()))
                .font(theme::label())
                .color(theme::FAINT),
        );
        // A shell that has gone says so and keeps its scrollback. Closing the pane is what
        // releases it, so whatever it printed on the way out can still be read.
        if !alive {
            ui.label(
                RichText::new("the shell has exited, its output is kept until you close this")
                    .font(theme::label())
                    .color(theme::WARN),
            );
        }
    });
    ui.add_space(4.0);

    ScrollArea::vertical()
        .id_salt("terminal-out")
        .stick_to_bottom(true)
        .max_height(HEIGHT - 150.0)
        .show(ui, |ui| {
            ui.label(
                RichText::new(&output)
                    .font(theme::body())
                    .color(theme::TEXT),
            );
        });

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(">").font(theme::body()).color(theme::ACCENT));
        let Some(Pane::Terminal { input, .. }) = app.workspace.as_mut().map(|w| &mut w.pane) else {
            return;
        };
        let response = ui.add_sized(
            [ui.available_width(), 24.0],
            TextEdit::singleline(input)
                .font(theme::body())
                .interactive(alive)
                .hint_text(if alive {
                    "type, then enter"
                } else {
                    "the shell has gone, nothing to type into"
                }),
        );
        if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            app.terminal_send();
            if let Some(w) = app.workspace.as_ref()
                && matches!(w.pane, Pane::Terminal { .. })
            {
                response.request_focus();
            }
        }
    });
}

fn editor(app: &mut App, ui: &mut eframe::egui::Ui) {
    app.editor_check_disk();

    let (read_only, changed, refused, conflict, path) =
        match app.workspace.as_ref().map(|w| w.pane.clone()) {
            Some(Pane::Editor {
                read_only,
                changed_on_disk,
                refused,
                conflict,
                path,
                ..
            }) => (read_only, changed_on_disk, refused, conflict, path),
            _ => return,
        };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&path)
                .font(theme::label())
                .color(theme::FAINT),
        );
        if read_only {
            widgets::state_chip(ui, widgets::Mark::Barred, "READ ONLY", theme::WARN);
        }
        // Somebody else has touched it. Said before a save is attempted rather than after.
        if changed {
            ui.label(
                RichText::new("changed on disk since it was opened")
                    .font(theme::label())
                    .color(theme::WARN),
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .button(RichText::new("RELOAD").font(theme::label()))
                .clicked()
            {
                app.editor_reload();
            }
            if ui
                .add_enabled(
                    !read_only,
                    eframe::egui::Button::new(RichText::new("SAVE").font(theme::label())),
                )
                .clicked()
            {
                app.editor_save();
            }
        });
    });

    // A refusal is the facade's answer, shown as it was given. A conflict gets more than that,
    // because it is the one refusal with a choice attached and nothing was overwritten.
    if let Some(why) = refused {
        if conflict {
            eframe::egui::Frame::none()
                .fill(theme::RAISED)
                .stroke(theme::edge(theme::WARN))
                .rounding(theme::CORNER)
                .inner_margin(eframe::egui::Margin::symmetric(8.0, 6.0))
                .show(ui, |ui| {
                    ui.label(
                        theme::spaced("NOT SAVED, THE FILE CHANGED UNDERNEATH")
                            .font(theme::label())
                            .color(theme::WARN),
                    );
                    ui.label(RichText::new(why).font(theme::label()).color(theme::DIM));
                    ui.label(
                        RichText::new(
                            "Nothing was written. Your text is still here. Reload to take what \
                             is on disk and lose your changes, or copy what you need out first.",
                        )
                        .font(theme::label())
                        .color(theme::TEXT),
                    );
                });
        } else {
            ui.label(RichText::new(why).font(theme::label()).color(theme::BAD));
        }
    }
    ui.add_space(4.0);

    ScrollArea::vertical()
        .id_salt("editor-buffer")
        .max_height(HEIGHT - 142.0)
        .show(ui, |ui| {
            let Some(Pane::Editor { buffer, .. }) = app.workspace.as_mut().map(|w| &mut w.pane)
            else {
                return;
            };
            ui.add_sized(
                [ui.available_width(), HEIGHT - 150.0],
                TextEdit::multiline(buffer).font(theme::body()),
            );
        });
}

/// A comparison, with the four outcomes drawn as four different things.
///
/// The one that matters is `Unavailable`. A repository with no commits cannot be compared at
/// all, and drawing that as a clean tree would tell somebody their work was committed when
/// nothing is.
fn comparison(ui: &mut eframe::egui::Ui, outcome: &Comparison) {
    match outcome {
        Comparison::Changes(text) => scroll(ui, "diff", text, theme::TEXT),
        Comparison::Same => {
            ui.label(
                RichText::new("no difference against HEAD")
                    .font(theme::body())
                    .color(theme::GOOD),
            );
            ui.label(
                RichText::new("compared successfully, and there is nothing to show")
                    .font(theme::label())
                    .color(theme::FAINT),
            );
        }
        Comparison::Binary => {
            ui.label(
                RichText::new("the files differ and git will not say how")
                    .font(theme::body())
                    .color(theme::COLD),
            );
            ui.label(
                RichText::new("binary, so there is no text to compare")
                    .font(theme::label())
                    .color(theme::FAINT),
            );
        }
        Comparison::Unavailable(why) => {
            ui.label(
                theme::spaced("COMPARISON UNAVAILABLE")
                    .font(theme::label())
                    .color(theme::WARN),
            );
            ui.label(RichText::new(why).font(theme::body()).color(theme::TEXT));
            ui.add_space(4.0);
            ui.label(
                RichText::new("this is not the same as having no changes")
                    .font(theme::label())
                    .color(theme::WARN),
            );
        }
    }
}

fn investigation(
    ui: &mut eframe::egui::Ui,
    found: &carl::providers::workspace::service::Investigation,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&found.component)
                .font(theme::heading())
                .color(theme::TEXT),
        );
        ui.add_space(10.0);
        widgets::state_chip(
            ui,
            widgets::health_mark(found.health),
            widgets::health_label(found.health),
            widgets::health_color(found.health),
        );
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(&found.summary)
            .font(theme::prose())
            .color(theme::DIM),
    );
    ui.add_space(8.0);

    if found.metrics.is_empty() {
        ui.label(
            RichText::new("no metrics were readable for this component")
                .font(theme::label())
                .color(theme::UNKNOWN),
        );
    }
    for (name, value) in &found.metrics {
        widgets::field(ui, name, Some(value));
    }
}

fn scroll(ui: &mut eframe::egui::Ui, id: &str, text: &str, color: eframe::egui::Color32) {
    ScrollArea::vertical()
        .id_salt(id)
        .max_height(HEIGHT - 96.0)
        .show(ui, |ui| {
            ui.label(RichText::new(text).font(theme::body()).color(color));
        });
}
