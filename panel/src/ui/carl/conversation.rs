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
                        let tick = ui.input(|i| i.time);
                        block(
                            ui,
                            who,
                            color,
                            turn,
                            &text_of(turn, tick),
                            widgets::ago(now, turn.at),
                        );
                    }
                });
        });
}

/// What a turn reads as, including while it is still arriving.
///
/// The caret is `|`, and the reason is a font rather than a taste.
///
/// It was `\u{2588}` and then `\u{258f}`, and neither was ever drawn. Ubuntu-Light, the
/// proportional face this text is rendered in, contains no Block Elements at all, so egui drew
/// its missing glyph box for both and JJ saw a square under Carl's name. Hack has them and this
/// text is not in Hack. Checking the font rather than picking a different block is the whole
/// fix: `|` is ASCII and is in every font there has ever been.
///
/// `tick` is the running time in seconds, used only to animate. A still indicator cannot be
/// told from a stuck one.
pub(crate) fn text_of(turn: &crate::model::Turn, tick: f64) -> String {
    if !turn.streaming {
        return turn.text.clone();
    }
    if turn.text.trim().is_empty() {
        // Nothing of the answer yet. The reasoning and the tool list are drawn above this and
        // usually say far more, so this only covers the moment before even those arrive.
        let dots = ".".repeat(1 + (tick * 2.0) as usize % 3);
        return format!("working{dots}");
    }
    // Blinking, so a live stream and a stalled one look different. The off frame is a space
    // rather than nothing, so the text does not shift by a character twice a second.
    match (tick * 2.0) as usize % 2 {
        0 => format!("{}|", turn.text),
        _ => format!("{} ", turn.text),
    }
}

/// One turn, as a block with the speaker's colour down its edge.
///
/// The reasoning and the tool list go above the answer, in a quieter colour. Above, because
/// they are produced first and reading them after the reply is pointless. Quieter, because
/// neither is the answer and the reply has to stay the thing your eye lands on.
fn block(
    ui: &mut Ui,
    who: &str,
    color: eframe::egui::Color32,
    turn: &crate::model::Turn,
    text: &str,
    when: String,
) {
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
            super::working::draw(ui, turn, turn.at);
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
