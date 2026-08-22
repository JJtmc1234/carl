//! What the agents are doing, in one compact block.
//!
//! Ordered by how much somebody should care rather than alphabetically or by rank. A screen
//! sorted by name puts the blocked worker below the idle one two thirds of the time, and this
//! block exists precisely so nobody has to read all of it.

use eframe::egui::{Align, Label, Layout, RichText, Ui, vec2};

use crate::app::{App, Tab};
use crate::model::{AgentStatus, AgentView};
use crate::theme;
use crate::ui::vitals;
use crate::ui::widgets;

/// How many fit before the block starts hiding people.
pub const SHOWN: usize = 7;

/// Everybody, worst first, with JJ left out because he is not one of them.
///
/// The order inside a status is the chain order the snapshot arrived in, which is stable, so
/// rows do not swap places under the pointer between frames.
pub fn on_deck(agents: &[AgentView]) -> Vec<&AgentView> {
    let mut out: Vec<&AgentView> = agents
        .iter()
        .filter(|a| !vitals::is_human(&a.name))
        .collect();
    out.sort_by_key(|a| rank_of(a.status));
    out
}

/// How far up the block a status sits. Blocked first, then anything moving, then the states
/// nobody has to do anything about, and unknown last of the ones that are real.
fn rank_of(status: AgentStatus) -> u8 {
    match status {
        AgentStatus::Blocked => 0,
        AgentStatus::AwaitingReview => 1,
        AgentStatus::Working => 2,
        AgentStatus::Idle => 3,
        AgentStatus::Unknown => 4,
    }
}

pub fn draw(app: &mut App, ui: &mut Ui) {
    let agents: Vec<AgentView> = on_deck(&app.snapshot.agents).into_iter().cloned().collect();
    widgets::section_count(ui, "THE ARMY", agents.len(), theme::DIM);

    if agents.is_empty() {
        ui.label(
            RichText::new("the backend has sent no agents")
                .font(theme::prose())
                .color(theme::UNKNOWN),
        );
        return;
    }

    let now = app.snapshot.at;
    let mut pick: Option<String> = None;
    for agent in agents.iter().take(SHOWN) {
        let response = widgets::card(
            ui,
            64.0,
            widgets::Card::default()
                .attention(agent.status.wants_attention())
                .selected(app.agent.as_deref() == Some(agent.name.as_str()))
                .lit(app.is_lit(&agent.name)),
            |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(10.0, 10.0), eframe::egui::Sense::hover());
                    widgets::mark(
                        ui.painter(),
                        rect,
                        widgets::status_mark(agent.status),
                        widgets::status_color(agent.status),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(&agent.name)
                            .font(theme::body())
                            .color(theme::TEXT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(agent.status.label())
                                .font(theme::label())
                                .color(widgets::status_color(agent.status)),
                        );
                        if let Some(at) = agent.last_activity_at {
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(widgets::ago(now, at))
                                    .font(theme::label())
                                    .color(theme::FAINT),
                            );
                        }
                    });
                });
                let (text, colour) = super::super::agents::work_line(agent);
                ui.add(
                    Label::new(RichText::new(text).font(theme::prose()).color(colour)).truncate(),
                );
            },
        );
        if response.clicked() {
            pick = Some(agent.name.clone());
        }
    }

    if agents.len() > SHOWN {
        ui.add_space(4.0);
        if super::super::shell::open_link(
            ui,
            &format!("{} more on the agents screen", agents.len() - SHOWN),
        ) {
            app.select_tab(Tab::Agents);
        }
    }

    if let Some(name) = pick {
        app.select_agent(&name);
        app.select_tab(Tab::Agents);
    }
}

#[cfg(test)]
mod tests;
