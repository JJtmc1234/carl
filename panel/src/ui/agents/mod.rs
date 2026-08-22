//! Who everyone is, who they answer to, and what they are doing, answerable in a couple of
//! seconds.
//!
//! The hierarchy is drawn from `army::org` by walking `reports_to`, so it is the real chain and
//! not a copy of it kept in the UI. Adding an agent to `org.rs` puts them on this screen with
//! no change here.
//!
//! **Cards and connectors, not indentation alone.** The old rule was the opposite and it is
//! cancelled. Indentation does carry a hierarchy, and a screen of indented one line rows
//! carrying it is `htop`. A card makes an agent a thing you can point at, and an elbow from
//! parent to child makes the reporting line a drawn fact rather than something inferred from
//! how far in a row starts.
//!
//! **JJ is not one of the cards.** He is the authority the whole army hangs off and he is not
//! part of it, so he sits above the tree in his own colour, with the line down into the army
//! drawn but the two never mixed. Counting him as an agent, or drawing him as one, is the
//! quickest way to make an organisation chart lie about itself.
//!
//! **It has to survive twenty plus.** Every subtree folds, the whole thing folds at once from
//! the toolbar, and the compact card is the default so a department of workers is a readable
//! list rather than twelve posters.

use std::collections::BTreeSet;

use eframe::egui::{Id, RichText, ScrollArea, Sense, Ui, vec2};

use crate::app::App;
use crate::model::{AgentStatus, AgentView};
use crate::theme;

use super::{detail, shell, widgets};

mod card;
pub mod layout;

#[cfg(test)]
mod tests;

/// Where the folded subtrees are remembered.
///
/// In the context rather than in `App`, deliberately. Which branches somebody has folded is a
/// fact about this window, not about the army, and putting it in the state the backend feeds
/// would be the first step towards the panel having opinions of its own.
fn memory_id() -> Id {
    Id::new("agents-collapsed")
}

fn collapsed(ui: &Ui) -> BTreeSet<String> {
    ui.data(|d| d.get_temp::<BTreeSet<String>>(memory_id()))
        .unwrap_or_default()
}

fn set_collapsed(ui: &mut Ui, folded: BTreeSet<String>) {
    ui.data_mut(|d| d.insert_temp(memory_id(), folded));
}

pub fn draw(app: &mut App, ui: &mut Ui) {
    let (left, right) = shell::columns_for(ui, 0.55);

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui(vec2(left, ui.available_height()), |ui| {
            ui.vertical(|ui| {
                authority(app, ui);
                ui.add_space(10.0);
                toolbar(ui);
                ui.add_space(6.0);
                tree(app, ui);
            });
        });
        ui.add_space(theme::GAP + 4.0);
        ui.allocate_ui(vec2(right, ui.available_height()), |ui| {
            ui.vertical(|ui| detail::draw(app, ui));
        });
    });
}

/// JJ, drawn apart from the army and in the colour used nowhere else.
fn authority(app: &mut App, ui: &mut Ui) {
    widgets::section(ui, "COMMAND AUTHORITY");
    for name in layout::command_authority() {
        let view = app
            .snapshot
            .agent(&name)
            .cloned()
            .unwrap_or_else(|| AgentView::unknown(&name));
        let selected = app.agent.as_deref() == Some(name.as_str());
        let response = widgets::card(
            ui,
            76.0,
            widgets::Card::default()
                .selected(selected)
                .tone(widgets::Tone::Authority),
            |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        theme::spaced(&view.name.to_uppercase())
                            .font(theme::heading())
                            .color(theme::INTERVENE),
                    );
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("the person, and not one of the agents")
                            .font(theme::prose())
                            .color(theme::DIM),
                    );
                });
                ui.label(
                    RichText::new(format!("delegates to {}", layout::army_roots().join(", ")))
                        .font(theme::label())
                        .color(theme::FAINT),
                );
            },
        );
        if response.clicked() {
            app.select_agent(&name);
        }
    }
}

/// The heading over the tree, with the two controls that make twenty agents workable.
fn toolbar(ui: &mut Ui) {
    let mut folded = collapsed(ui);
    ui.horizontal(|ui| {
        ui.label(
            theme::spaced("CHAIN OF COMMAND")
                .font(theme::label())
                .color(theme::DIM),
        );
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                if ui
                    .button(RichText::new("EXPAND ALL").font(theme::label()))
                    .clicked()
                {
                    folded.clear();
                }
                if ui
                    .button(RichText::new("FOLD DEPARTMENTS").font(theme::label()))
                    .clicked()
                {
                    folded = every_manager();
                }
            },
        );
    });
    set_collapsed(ui, folded);
}

/// Everybody with somebody under them, which is what folding everything means.
fn every_manager() -> BTreeSet<String> {
    carl::army::org::everyone()
        .iter()
        .filter(|a| a.rank != carl::army::org::Rank::Human)
        .filter(|a| !layout::reports_of(a.name).is_empty())
        .map(|a| a.name.to_string())
        .collect()
}

/// Everybody, in the order the chain puts them, as cards joined by their reporting lines.
fn tree(app: &mut App, ui: &mut Ui) {
    let folded = collapsed(ui);
    let nodes = layout::arrange(&layout::army_roots(), &layout::reports_of, &folded);
    let views: Vec<AgentView> = nodes
        .iter()
        .map(|n| {
            app.snapshot
                .agent(&n.name)
                .cloned()
                .unwrap_or_else(|| AgentView::unknown(&n.name))
        })
        .collect();
    let heights: Vec<f32> = views
        .iter()
        .map(|v| layout::card_height(v.rank(), v.status.wants_attention()))
        .collect();

    let mut pick: Option<String> = None;
    let mut fold: Option<String> = None;

    ScrollArea::vertical()
        .id_salt("agent-tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width() - 14.0;
            let origin = ui.cursor().left_top();
            let (placed, total) = layout::place(&nodes, &heights, origin, width);
            ui.allocate_exact_size(vec2(width, total.max(1.0)), Sense::hover());

            // Connectors first, so a card always sits on top of the line into it rather than
            // the line crossing the card's edge.
            for p in &placed {
                let Some(parent) = p.parent else { continue };
                let spine = layout::spine_x(origin.x, nodes[p.at].depth);
                widgets::connector(
                    ui.painter(),
                    spine,
                    placed[parent].rect.bottom(),
                    p.rect.center().y,
                    p.rect.left(),
                );
            }

            for p in &placed {
                let node = &nodes[p.at];
                let view = &views[p.at];
                let selected = app.agent.as_deref() == Some(node.name.as_str());
                let outcome = card::draw(
                    ui,
                    p.rect,
                    node,
                    view,
                    selected,
                    app.is_lit(&node.name),
                    app.snapshot.at,
                );
                match outcome {
                    card::Hit::Fold => fold = Some(node.name.clone()),
                    card::Hit::Select => pick = Some(node.name.clone()),
                    card::Hit::Nothing => {}
                }
            }
        });

    if let Some(name) = fold {
        let mut next = collapsed(ui);
        if !next.remove(&name) {
            next.insert(name);
        }
        set_collapsed(ui, next);
    }
    if let Some(name) = pick {
        app.select_agent(&name);
    }
}

/// A short description of what somebody is, for the line next to their name.
pub fn role_of(view: &AgentView) -> String {
    match (&view.sub_department, &view.department) {
        (Some(sub), Some(dept)) => format!("{dept} / {sub}"),
        (Some(sub), None) => sub.clone(),
        (None, Some(dept)) => dept.clone(),
        (None, None) => view
            .rank()
            .map(|r| r.to_string())
            .unwrap_or_else(|| "role not recorded".into()),
    }
}

/// The one line that says what this agent is doing, and how to colour it.
///
/// A blocker beats an activity, an activity beats silence, and silence says so rather than
/// leaving the line empty. A blank line there reads as a rendering fault, and a rendering
/// fault is indistinguishable from an agent nobody has heard from.
pub fn work_line(view: &AgentView) -> (String, eframe::egui::Color32) {
    match (&view.blocker, &view.last_activity) {
        (Some(b), _) => (b.clone(), theme::BAD),
        (None, Some(a)) => (a.clone(), theme::DIM),
        (None, None) => match view.status {
            AgentStatus::Unknown => ("nothing has reported on this agent".into(), theme::UNKNOWN),
            _ => ("no activity recorded".into(), theme::UNKNOWN),
        },
    }
}
