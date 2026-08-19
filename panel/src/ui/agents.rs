//! What everyone is doing, answerable in a couple of seconds.
//!
//! The hierarchy is drawn from `army::org` by walking `reports_to`, so it is the real chain
//! and not a copy of it kept in the UI. Adding an agent to `org.rs` puts them on this screen
//! with no change here.
//!
//! Indentation carries the hierarchy and nothing else does. No connector lines, no boxes: at
//! four levels deep the indent is unambiguous, and the ink saved goes to the status column,
//! which is what somebody is actually here to read.

use eframe::egui::{Align, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::model::{AgentStatus, AgentView};
use crate::theme;

use super::{detail, shell, widgets};

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.46);

    ui.horizontal_top(|ui| {
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| tree(app, ui));
        });
        ui.add_space(16.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| detail::draw(app, ui));
        });
    });
}

/// Everybody, in the order the chain puts them.
fn tree(app: &mut App, ui: &mut Ui) {
    widgets::section(ui, "CHAIN OF COMMAND");

    let order = walk();
    let now = app.snapshot.at;

    ScrollArea::vertical().id_salt("agent-tree").show(ui, |ui| {
        for (name, depth) in order {
            let view = app
                .snapshot
                .agent(&name)
                .cloned()
                .unwrap_or_else(|| AgentView::unknown(&name));
            let selected = app.agent.as_deref() == Some(name.as_str());
            let lit = app.is_lit(&name);

            let response = widgets::row(ui, 46.0, selected, lit, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 16.0);
                    widgets::pip(
                        ui,
                        widgets::status_color(view.status),
                        view.status != AgentStatus::Unknown,
                    );
                    ui.label(
                        RichText::new(&view.name)
                            .font(theme::body())
                            .color(if selected { theme::ACCENT } else { theme::TEXT }),
                    );
                    ui.label(
                        RichText::new(role_of(&view))
                            .font(theme::label())
                            .color(theme::FAINT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(view.status.label())
                                .font(theme::label())
                                .color(widgets::status_color(view.status)),
                        );
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 16.0 + 15.0);
                    let line = match (&view.blocker, &view.last_activity) {
                        (Some(b), _) => RichText::new(b.as_str())
                            .font(theme::label())
                            .color(theme::BAD),
                        (None, Some(a)) => RichText::new(a.as_str())
                            .font(theme::label())
                            .color(theme::DIM),
                        (None, None) => RichText::new("no activity recorded")
                            .font(theme::label())
                            .color(theme::UNKNOWN),
                    };
                    ui.label(line);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(at) = view.last_activity_at {
                            ui.label(
                                RichText::new(widgets::ago(now, at))
                                    .font(theme::label())
                                    .color(theme::FAINT),
                            );
                        }
                    });
                });
            });

            if response.clicked() {
                app.select_agent(&name);
            }
        }
    });
}

/// The real hierarchy, depth first from whoever answers to nobody.
///
/// Read out of `army::org` every time rather than cached, because the panel showing a stale
/// shape of the organisation is exactly the class of lie it exists to prevent.
pub fn walk() -> Vec<(String, usize)> {
    fn under(name: &str, depth: usize, out: &mut Vec<(String, usize)>) {
        out.push((name.to_string(), depth));
        let mut below = carl::army::org::reports_of(name);
        below.sort_by_key(|a| a.name);
        for agent in below {
            under(agent.name, depth + 1, out);
        }
    }

    let mut out = Vec::new();
    for root in carl::army::org::everyone().iter().filter(|a| a.is_root()) {
        under(root.name, 0, &mut out);
    }
    out
}

/// A short description of what somebody is, for the line next to their name.
fn role_of(view: &AgentView) -> String {
    match (&view.sub_department, &view.department) {
        (Some(sub), _) => sub.clone(),
        (None, Some(dept)) => dept.clone(),
        (None, None) => view
            .rank()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "unknown".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree must be the real chain, in order, at the right depths. Hardcoding a second
    /// hierarchy in the UI is the thing this test exists to catch.
    #[test]
    fn the_tree_is_the_real_chain() {
        let walked = walk();
        let names: Vec<&str> = walked.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["jj", "carl", "adrian", "mason", "nora"]);

        let depths: Vec<usize> = walked.iter().map(|(_, d)| *d).collect();
        assert_eq!(depths, vec![0, 1, 2, 3, 4], "one indent per level");
    }

    /// Everybody in the organisation appears exactly once, so nobody is invisible and nobody
    /// is drawn twice.
    #[test]
    fn everybody_appears_once() {
        let walked = walk();
        assert_eq!(walked.len(), carl::army::org::everyone().len());

        let mut names: Vec<&str> = walked.iter().map(|(n, _)| n.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "somebody is drawn twice");
    }
}
