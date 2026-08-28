//! One agent, on one card.
//!
//! Four things have to be readable without stopping: who it is, what it is for, what it is
//! doing or what has stopped it, and whether anybody has to act. Everything else belongs in
//! the inspector, and putting it here instead is how a card grows into a page.

use eframe::egui::{Align, Label, Layout, Rect, RichText, Sense, Ui, vec2};

use crate::model::AgentView;
use crate::theme;
use crate::ui::widgets::{self, Card, Mark};

use super::layout::Node;

/// What the pointer did to a card.
pub enum Hit {
    Nothing,
    Select,
    /// The fold control, which is a different act from picking the agent.
    Fold,
}

pub fn draw(
    ui: &mut Ui,
    rect: Rect,
    node: &Node,
    view: &AgentView,
    selected: bool,
    lit: bool,
    now: u64,
) -> Hit {
    let attention = view.status.wants_attention();
    let mut folded_clicked = false;

    let response = widgets::sized_card(
        ui,
        rect,
        Card::default()
            .selected(selected)
            .attention(attention)
            .lit(lit),
        |ui| {
            ui.horizontal(|ui| {
                folded_clicked = chevron(ui, node);
                ui.add_space(4.0);
                let (mark_rect, _) = ui.allocate_exact_size(vec2(11.0, 11.0), Sense::hover());
                widgets::mark(
                    ui.painter(),
                    mark_rect,
                    widgets::status_mark(view.status),
                    widgets::status_color(view.status),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&view.name)
                        .font(theme::heading())
                        .color(if selected { theme::ACCENT } else { theme::TEXT }),
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

            ui.horizontal(|ui| {
                ui.add_space(CHEVRON_WIDTH + 19.0);
                ui.add(
                    Label::new(
                        RichText::new(super::role_of(view))
                            .font(theme::label())
                            .color(theme::FAINT),
                    )
                    .truncate(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if node.collapsed {
                        widgets::state_chip(
                            ui,
                            Mark::Hollow,
                            &format!("{} FOLDED", node.hidden),
                            theme::COLD,
                        );
                    } else if node.reports > 0 {
                        ui.label(
                            RichText::new(format!("{} reporting", node.reports))
                                .font(theme::label())
                                .color(theme::FAINT),
                        );
                    }
                });
            });

            ui.add_space(2.0);
            work(ui, view, attention, now);
        },
    );

    if folded_clicked {
        Hit::Fold
    } else if response.clicked() {
        Hit::Select
    } else {
        Hit::Nothing
    }
}

/// How much room the fold control takes, so the lines under it line up with the name and not
/// with the control.
///
/// The same whether or not there is a control to draw. A leaf that reclaimed the space would
/// sit sixteen pixels left of its siblings, and the eye reads that as a different depth in the
/// tree rather than as a tidier margin.
const CHEVRON_WIDTH: f32 = 16.0;

/// The fold control. Drawn rather than typed, because a triangle from a font is one missing
/// glyph away from being an empty box.
fn chevron(ui: &mut Ui, node: &Node) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(CHEVRON_WIDTH, 16.0), Sense::click());
    if !node.can_collapse() {
        return false;
    }
    let color = if response.hovered() {
        theme::ACCENT
    } else {
        theme::DIM
    };
    let c = rect.center();
    let points = if node.collapsed {
        vec![
            c + vec2(-3.0, -5.0),
            c + vec2(-3.0, 5.0),
            c + vec2(4.0, 0.0),
        ]
    } else {
        vec![
            c + vec2(-5.0, -3.0),
            c + vec2(5.0, -3.0),
            c + vec2(0.0, 4.0),
        ]
    };
    ui.painter().add(eframe::egui::Shape::convex_polygon(
        points,
        color,
        eframe::egui::Stroke::NONE,
    ));
    response.clicked()
}

/// What this agent is doing, or what has stopped it.
fn work(ui: &mut Ui, view: &AgentView, attention: bool, now: u64) {
    let (text, color) = super::work_line(view);

    // The right hand side says when, or says that nothing said when. Worked out first and
    // measured, because the room left for the activity line used to be a constant 80 and
    // "no time recorded" is 128 wide at this type scale, so the two labels met in the middle.
    let (when, when_ink) = match view.last_activity_at {
        Some(at) => (widgets::ago(now, at), theme::FAINT),
        // Never a blank and never a zero. Nothing said when, so the card says that.
        None => ("no time recorded".to_string(), theme::UNKNOWN),
    };
    let when_width = ui.fonts(|f| {
        f.layout_no_wrap(when.clone(), theme::label(), eframe::egui::Color32::WHITE)
            .size()
            .x
    });

    ui.horizontal(|ui| {
        ui.add_space(CHEVRON_WIDTH + 19.0);
        let width = ui.available_width() - when_width - theme::PAD;
        ui.allocate_ui(
            vec2(width.max(60.0), if attention { 44.0 } else { 22.0 }),
            |ui| {
                let label = Label::new(RichText::new(text).font(theme::prose()).color(color));
                // A blocker is the reason somebody is reading this card, so it is allowed to wrap
                // onto the second line the taller card was given. Everything else is truncated,
                // because a wrapped activity line would push the card out of its own rectangle.
                ui.add(if attention {
                    label.wrap()
                } else {
                    label.truncate()
                });
            },
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(when).font(theme::label()).color(when_ink));
        });
    });
}
