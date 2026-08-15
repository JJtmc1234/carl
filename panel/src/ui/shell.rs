//! The frame everything else sits in.
//!
//! A fixed left rail, a thin status strip along the top, the tab underneath, and the
//! contextual workspace docked at the bottom when something has opened it.
//!
//! The rail is narrow and always there. Four destinations is few enough that hiding them
//! behind a menu would save nothing and cost the one thing a rail is good at, which is telling
//! you where you are without being read.

use eframe::egui::{
    Align, CentralPanel, Context, Layout, RichText, Sense, SidePanel, TopBottomPanel, vec2,
};

use crate::app::{App, Tab};
use crate::theme;

use super::widgets;

pub fn draw(app: &mut App, ctx: &Context) {
    rail(app, ctx);
    strip(app, ctx);
    if app.workspace.is_some() {
        super::workspace::draw(app, ctx);
    }
    CentralPanel::default().show(ctx, |ui| {
        ui.add_space(4.0);
        match app.tab {
            Tab::Carl => super::carl::draw(app, ui),
            Tab::Agents => super::agents::draw(app, ui),
            Tab::Diagnostics => super::diagnostics::draw(app, ui),
            Tab::Projects => super::projects::draw(app, ui),
        }
    });
}

/// The persistent left sidebar.
fn rail(app: &mut App, ctx: &Context) {
    SidePanel::left("rail")
        .exact_width(186.0)
        .resizable(false)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::PANEL)
                .inner_margin(eframe::egui::Margin::symmetric(12.0, 14.0)),
        )
        .show(ctx, |ui| {
            ui.label(
                RichText::new(theme::spaced("AOS"))
                    .font(theme::big())
                    .color(theme::TEXT),
            );
            ui.label(
                RichText::new(theme::spaced("COMMAND PANEL"))
                    .font(theme::label())
                    .color(theme::FAINT),
            );
            ui.add_space(18.0);

            for tab in Tab::ALL {
                let selected = app.tab == tab;
                let attention = wants_attention(app, tab);
                let response = widgets::row(ui, 38.0, selected, false, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(theme::spaced(tab.label()))
                                .font(theme::body())
                                .color(if selected { theme::ACCENT } else { theme::TEXT }),
                        );
                        if attention > 0 {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(
                                    RichText::new(attention.to_string())
                                        .font(theme::label())
                                        .color(theme::BAD),
                                );
                            });
                        }
                    });
                    ui.label(
                        RichText::new(tab.caption())
                            .font(theme::label())
                            .color(theme::FAINT),
                    );
                });
                if response.clicked() {
                    app.select_tab(tab);
                }
                ui.add_space(2.0);
            }

            // The rail's foot carries the things that are true regardless of tab.
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(app.source_name())
                        .font(theme::label())
                        .color(theme::FAINT),
                );
                ui.add_space(6.0);
                widgets::link_badge(ui, &app.link);
                ui.add_space(10.0);
                ui.label(
                    RichText::new("F9  hide or show")
                        .font(theme::label())
                        .color(theme::FAINT),
                );
            });
        });
}

/// How many things on a tab want somebody's attention, which is what the rail counts.
fn wants_attention(app: &App, tab: Tab) -> usize {
    match tab {
        Tab::Carl => app.snapshot.decisions.len(),
        Tab::Agents => app
            .snapshot
            .agents
            .iter()
            .filter(|a| a.status.wants_attention())
            .count(),
        Tab::Diagnostics => app
            .snapshot
            .diagnostics
            .iter()
            .filter(|d| widgets::wants_attention(d.health))
            .count(),
        Tab::Projects => app
            .snapshot
            .projects
            .iter()
            .filter(|p| !p.project.blockers.is_empty())
            .count(),
    }
}

/// The thin strip along the top: where you are, and whether what you are seeing is real.
fn strip(app: &mut App, ctx: &Context) {
    TopBottomPanel::top("strip")
        .exact_height(34.0)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::PANEL)
                .inner_margin(eframe::egui::Margin::symmetric(14.0, 8.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(theme::spaced(app.tab.label()))
                        .font(theme::body())
                        .color(theme::TEXT),
                );

                if let Some(warning) = stale_warning(app) {
                    ui.add_space(14.0);
                    ui.label(
                        RichText::new(warning)
                            .font(theme::label())
                            .color(theme::BAD),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some((text, ok)) = &app.notice {
                        ui.label(RichText::new(text).font(theme::label()).color(if *ok {
                            theme::DIM
                        } else {
                            theme::BAD
                        }));
                        ui.add_space(12.0);
                    }
                    if let Some(at) = app.resynced_at
                        && at.elapsed().as_secs() < 6
                    {
                        ui.label(
                            RichText::new("resynchronised")
                                .font(theme::label())
                                .color(theme::GOOD),
                        );
                    }
                });
            });
        });
}

/// The sentence that appears the moment the panel stops being live.
///
/// Blunt on purpose. Everything below it is a version of the world from before the link went,
/// and the one thing a panel must never do is let that look current.
fn stale_warning(app: &App) -> Option<String> {
    match &app.link {
        crate::model::Link::Live => None,
        crate::model::Link::Connecting { attempt } => Some(format!(
            "NOT LIVE, everything below is from before the link dropped, reconnecting {attempt}"
        )),
        crate::model::Link::Disconnected { why } => Some(format!(
            "NOT LIVE, everything below is from before the link dropped, {why}"
        )),
    }
}

/// Draws a heading and returns the space left, for tabs that split into columns.
pub fn columns_for(ui: &mut eframe::egui::Ui, left_fraction: f32) -> (f32, f32) {
    let total = ui.available_width();
    let gap = 16.0;
    let left = ((total - gap) * left_fraction).floor();
    (left, total - gap - left)
}

/// A clickable area that reads as a link rather than a button, for the many places the panel
/// offers to open something in the workspace.
pub fn open_link(ui: &mut eframe::egui::Ui, text: &str) -> bool {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::label(), theme::COLD);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x, galley.size().y + 2.0), Sense::click());
    let color = if response.hovered() {
        theme::ACCENT
    } else {
        theme::COLD
    };
    ui.painter().text(
        rect.left_top(),
        eframe::egui::Align2::LEFT_TOP,
        text,
        theme::label(),
        color,
    );
    if response.hovered() {
        ui.painter().hline(
            rect.left()..=rect.right(),
            rect.bottom() - 1.0,
            theme::edge(color),
        );
    }
    response.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AgentStatus, Health, Link};
    use crate::source::MockPanelDataSource;

    fn app() -> App {
        App::new(Box::new(MockPanelDataSource::new()))
    }

    /// The rail counts what needs somebody, which is how a tab you are not looking at tells
    /// you to look at it.
    #[test]
    fn the_rail_counts_what_needs_attention() {
        let mut a = app();
        assert_eq!(wants_attention(&a, Tab::Agents), 0);

        a.snapshot.agents[2].status = AgentStatus::Blocked;
        assert_eq!(wants_attention(&a, Tab::Agents), 1);

        // Unknown is a gap rather than a fault, so it must not be counted as one.
        a.snapshot.agents[3].status = AgentStatus::Unknown;
        assert_eq!(wants_attention(&a, Tab::Agents), 1);

        let degraded = a
            .snapshot
            .diagnostics
            .iter()
            .filter(|d| widgets::wants_attention(d.health))
            .count();
        assert_eq!(wants_attention(&a, Tab::Diagnostics), degraded);
        assert!(
            a.snapshot
                .diagnostics
                .iter()
                .any(|d| d.health == Health::Unknown),
            "the fixture must include an unmeasured component"
        );
    }

    /// The moment the link goes, the panel has to say that what is on screen is old.
    #[test]
    fn losing_the_link_puts_a_warning_across_the_top() {
        let mut a = app();
        assert_eq!(stale_warning(&a), None, "nothing to say while live");

        a.link = Link::Disconnected {
            why: "backend closed the connection".into(),
        };
        let w = stale_warning(&a).expect("a warning");
        assert!(w.contains("NOT LIVE"), "{w}");
        assert!(w.contains("before the link dropped"), "{w}");

        a.link = Link::Connecting { attempt: 2 };
        let w = stale_warning(&a).expect("a warning");
        assert!(w.contains("reconnecting 2"), "{w}");
    }
}
