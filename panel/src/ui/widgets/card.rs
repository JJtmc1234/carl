//! Cards, and the lines that join them.
//!
//! The old rule was no boxes and no connector lines, on the argument that indentation alone
//! carries a hierarchy and the ink saved goes to the data. It does carry the hierarchy, and
//! the result read as `ps` output. A card is what makes a thing on the screen an object you
//! can point at, and a connector is what makes the relationship between two of them a fact
//! rather than an inference from how far in they start.

use eframe::egui::{
    Align, Color32, Layout, Painter, Rect, Response, Sense, Stroke, Ui, pos2, vec2,
};

use crate::theme;

/// What kind of thing the card is, which decides its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    /// An ordinary object: an agent, a component, a project.
    #[default]
    Normal,
    /// Background material. A recent activity line, a delegation.
    Quiet,
    /// JJ. Outside the operational army, and drawn in the colour used nowhere else.
    Authority,
}

impl Tone {
    fn edge(self) -> Color32 {
        match self {
            Tone::Normal => theme::RULE,
            Tone::Quiet => theme::RULE,
            Tone::Authority => theme::INTERVENE,
        }
    }

    fn fill(self) -> Color32 {
        match self {
            Tone::Normal | Tone::Authority => theme::RAISED,
            Tone::Quiet => theme::PANEL,
        }
    }
}

/// How a card should be drawn. Everything here is state, never decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Card {
    pub selected: bool,
    /// Something on this card needs somebody. Earns the halo and the brighter edge.
    pub attention: bool,
    /// Changed a moment ago.
    pub lit: bool,
    pub tone: Tone,
}

impl Card {
    pub fn selected(mut self, yes: bool) -> Self {
        self.selected = yes;
        self
    }

    pub fn attention(mut self, yes: bool) -> Self {
        self.attention = yes;
        self
    }

    pub fn lit(mut self, yes: bool) -> Self {
        self.lit = yes;
        self
    }

    pub fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
}

/// A card filling the width, at a fixed height.
pub fn card(ui: &mut Ui, height: f32, spec: Card, add: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    sized_card(ui, rect, spec, add)
}

/// A card exactly as tall as what goes in it.
///
/// `card` takes a height, which is right for a grid of equal tiles and wrong for anything
/// holding a sentence somebody else wrote. A guessed constant fits the text it was guessed
/// against and clips the next one, which is what it did to the current task and to Carl's
/// question. This draws the content first, learns how tall it turned out, and paints the frame
/// behind it afterwards, so the card cannot be smaller than its own contents.
pub fn fitted_card(ui: &mut Ui, spec: Card, add: impl FnOnce(&mut Ui)) -> Response {
    let width = ui.available_width();
    let top_left = ui.cursor().min;

    // Room in the paint list, filled in once the height is known. Painting the frame first
    // would mean guessing the height again, which is the bug.
    let backdrop = ui.painter().add(eframe::egui::Shape::Noop);
    let edging = ui.painter().add(eframe::egui::Shape::Noop);

    // A pixel narrower than the room actually available. Text layout rounds up, so a paragraph
    // wrapped to exactly the inner width can finish a fraction past it and be reported as cut
    // off by its own card. Losing one pixel of line length is not a thing anybody can see.
    let inner = Rect::from_min_size(
        top_left + vec2(theme::PAD, 10.0),
        vec2((width - theme::PAD * 2.0 - 1.0).max(1.0), 0.0),
    );
    let mut child = ui.new_child(
        eframe::egui::UiBuilder::new()
            .max_rect(Rect::from_min_size(
                inner.min,
                vec2(inner.width(), f32::INFINITY),
            ))
            .layout(eframe::egui::Layout::top_down(eframe::egui::Align::Min)),
    );
    add(&mut child);
    let used = child.min_rect().height();

    let rect = Rect::from_min_size(top_left, vec2(width, used + 20.0));
    let (_, _) = ui.allocate_exact_size(rect.size(), Sense::hover());

    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let response = ui.interact(rect, id, Sense::click());
    let hovered = response.hovered();

    let fill = if spec.selected {
        theme::HOVER
    } else if hovered {
        theme::HOVER.gamma_multiply(0.8)
    } else {
        spec.tone.fill()
    };
    let edge = if spec.selected {
        theme::ACCENT
    } else if spec.attention {
        theme::BAD
    } else if hovered {
        theme::RULE_BRIGHT
    } else {
        spec.tone.edge()
    };

    ui.painter().set(
        backdrop,
        eframe::egui::Shape::rect_filled(rect, theme::CARD_CORNER, fill),
    );
    ui.painter().set(
        edging,
        eframe::egui::Shape::rect_stroke(rect, theme::CARD_CORNER, theme::edge(edge)),
    );
    response
}

/// A card at an exact rectangle, for the screens that place their own cards.
///
/// Drawing at a rectangle the caller worked out is what lets the agents tree compute every
/// position before it paints anything, which is the only way the connectors can be drawn
/// between cards that have not been laid out yet.
pub fn sized_card(ui: &mut Ui, rect: Rect, spec: Card, add: impl FnOnce(&mut Ui)) -> Response {
    let id = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    let response = ui.interact(rect, id, Sense::click());
    let hovered = response.hovered();

    let fill = if spec.selected {
        theme::HOVER
    } else if hovered {
        theme::HOVER.gamma_multiply(0.8)
    } else {
        spec.tone.fill()
    };
    let edge = if spec.selected {
        theme::ACCENT
    } else if spec.attention {
        theme::BAD
    } else if hovered {
        theme::RULE_BRIGHT
    } else {
        spec.tone.edge()
    };

    let painter = ui.painter();
    if spec.attention || spec.selected {
        theme::glow(
            painter,
            rect,
            if spec.selected { theme::ACCENT } else { edge },
            theme::CARD_CORNER,
        );
    }
    painter.rect_filled(rect, theme::CARD_CORNER, fill);
    painter.rect_stroke(rect, theme::CARD_CORNER, theme::edge(edge));

    // A left bar rather than a second border, so selection and a change that has just landed
    // speak the same language and neither of them changes the shape of the card.
    if spec.selected || spec.lit {
        let bar = Rect::from_min_size(
            rect.left_top() + vec2(1.0, 1.0),
            vec2(3.0, rect.height() - 2.0),
        );
        painter.rect_filled(
            bar,
            theme::CARD_CORNER,
            if spec.selected {
                theme::ACCENT
            } else {
                theme::ACCENT_DIM
            },
        );
    }

    // Same one pixel of slack as fitted_card, for the same rounding reason.
    let inner = rect.shrink2(vec2(theme::PAD + 1.0, 10.0));
    let mut child = ui.new_child(
        eframe::egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(inner);
    add(&mut child);
    response
}

/// The vertical part of a hierarchy connector.
pub fn spine(painter: &Painter, x: f32, from_y: f32, to_y: f32) {
    painter.line_segment(
        [pos2(x, from_y), pos2(x, to_y)],
        Stroke::new(1.0_f32, theme::RULE_BRIGHT),
    );
}

/// One reporting line, drawn as an elbow from the spine into the left edge of a card.
///
/// The elbow is what makes the relationship readable rather than inferred. `spine_x` is the
/// column the parent hands work down, `y` is the middle of the child's card, and the line
/// stops a couple of pixels short of the card so it touches the edge rather than crossing it.
pub fn connector(painter: &Painter, spine_x: f32, from_y: f32, y: f32, card_left: f32) {
    spine(painter, spine_x, from_y, y);
    painter.line_segment(
        [pos2(spine_x, y), pos2(card_left - 2.0, y)],
        Stroke::new(1.0_f32, theme::RULE_BRIGHT),
    );
}
