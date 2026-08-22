//! The conversation itself, as a surface rather than as a log.
//!
//! Each turn is a block with the speaker's colour down its left edge, on the raised surface, so
//! the exchange reads as an exchange. The old version was speaker labels and paragraphs on the
//! page background, which is what a terminal transcript looks like.

use eframe::egui::{Align, Label, Layout, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::model::Speaker;
use crate::theme;
use crate::ui::widgets;

pub fn draw(app: &mut App, ui: &mut Ui, asked: usize) {
    let now = app.snapshot.at;
    let turns = app.snapshot.conversation.clone();

    eframe::egui::Frame::none()
        .fill(theme::PANEL)
        .stroke(theme::hairline())
        .rounding(theme::CARD_CORNER)
        .inner_margin(eframe::egui::Margin::same(theme::PAD))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            ui.horizontal(|ui| {
                widgets::small(ui, "CONVERSATION");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if asked > 0 {
                        ui.label(
                            RichText::new(format!("{asked} question(s) waiting above"))
                                .font(theme::label())
                                .color(theme::ACCENT),
                        );
                    }
                });
            });
            ui.add_space(8.0);

            ScrollArea::vertical()
                .id_salt("conversation")
                .stick_to_bottom(app.conversation_at_end)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if turns.is_empty() {
                        empty(ui);
                        return;
                    }
                    for turn in &turns {
                        let (who, color) = match turn.from {
                            Speaker::Jj => ("JJ", theme::INTERVENE),
                            Speaker::Carl => ("CARL", theme::ACCENT),
                        };
                        block(ui, who, color, &text_of(turn), widgets::ago(now, turn.at));
                    }
                });
        });
}

fn text_of(turn: &crate::model::Turn) -> String {
    if turn.streaming {
        // A caret while the words are still arriving, so a half written answer is never
        // mistaken for a finished one.
        format!("{}\u{2588}", turn.text)
    } else {
        turn.text.clone()
    }
}

/// One turn, as a block with the speaker's colour down its edge.
fn block(ui: &mut Ui, who: &str, color: eframe::egui::Color32, text: &str, when: String) {
    let inner = eframe::egui::Frame::none()
        .fill(theme::RAISED)
        .rounding(theme::CARD_CORNER)
        .inner_margin(eframe::egui::Margin {
            left: 18.0,
            right: 14.0,
            top: 11.0,
            bottom: 11.0,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::spaced(who).font(theme::label()).color(color));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(when).font(theme::label()).color(theme::FAINT));
                });
            });
            ui.add_space(4.0);
            ui.add(Label::new(
                RichText::new(text).font(theme::prose()).color(theme::TEXT),
            ));
        });

    // The speaker's colour down the left edge, painted once the block's height is known.
    let rect = inner.response.rect;
    ui.painter().rect_filled(
        eframe::egui::Rect::from_min_size(rect.left_top(), vec2(4.0, rect.height())),
        theme::CARD_CORNER,
        color,
    );
    ui.add_space(10.0);
}

/// The empty state, which says the true thing rather than nothing.
///
/// A blank pane and a pane holding a conversation that was never loaded look the same and mean
/// opposite things. Blank reads as "Carl has said nothing", when what is true is "nothing from
/// before this panel opened is here". Inventing the earlier turns from the army's record would
/// be worse, because a delegation is not a thing Carl said.
fn empty(ui: &mut Ui) {
    ui.add_space(20.0);
    super::nothing_yet(
        ui,
        "NOTHING FROM BEFORE THIS PANEL OPENED",
        "Carl's earlier conversations happened on the surfaces they happened on, and this panel \
         does not receive them. Anything you say below starts a record here.",
    );
    ui.add_space(14.0);
    ui.label(
        RichText::new("Try one of these")
            .font(theme::label())
            .color(theme::FAINT),
    );
    ui.add_space(4.0);
    for suggestion in [
        "Ask what the army is working on right now.",
        "Set an objective, which goes to the departments rather than to one agent.",
        "Ask why something is blocked, and who is holding it.",
    ] {
        ui.label(
            RichText::new(format!("  {suggestion}"))
                .font(theme::prose())
                .color(theme::DIM),
        );
    }
}
