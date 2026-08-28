//! The shapes a state is drawn as.
//!
//! Five of them, each distinguishable in silhouette, so a state is carried by the shape as
//! well as by the hue. Two states that share a colour, degraded and blocked, must never share
//! a shape, and unknown is a bare line rather than any kind of box because it is the absence
//! of a state rather than one of them.

use eframe::egui::{Color32, Painter, Rect, Stroke, pos2};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// Working, or healthy. A solid block.
    Filled,
    /// Part way. A block filled from the bottom to halfway.
    Half,
    /// Held up by something. An outline with a bar straight through it.
    Barred,
    /// Gone wrong. A cross, no box.
    Cross,
    /// Nothing to do, and that is fine. An empty outline.
    Hollow,
    /// Nobody has said. A single line, which reads as a gap rather than as a state.
    Dash,
}

impl Mark {
    /// A one word name, used by the tests and by anything that has to describe a mark in text.
    pub fn name(self) -> &'static str {
        match self {
            Mark::Filled => "filled",
            Mark::Half => "half",
            Mark::Barred => "barred",
            Mark::Cross => "cross",
            Mark::Hollow => "hollow",
            Mark::Dash => "dash",
        }
    }
}

/// Draws a mark inside the given square.
pub fn mark(painter: &Painter, rect: Rect, shape: Mark, color: Color32) {
    let stroke = Stroke::new(1.5_f32, color);
    match shape {
        Mark::Filled => {
            painter.rect_filled(rect, 1.0_f32, color);
        }
        Mark::Half => {
            painter.rect_stroke(rect, 1.0_f32, theme::edge(color));
            let bottom = Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.max);
            painter.rect_filled(bottom, 0.0_f32, color);
        }
        Mark::Barred => {
            painter.rect_stroke(rect, 1.0_f32, theme::edge(color));
            painter.hline(rect.left()..=rect.right(), rect.center().y, stroke);
        }
        Mark::Cross => {
            painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
            painter.line_segment([rect.left_bottom(), rect.right_top()], stroke);
        }
        Mark::Hollow => {
            painter.rect_stroke(rect, 1.0_f32, theme::edge(color));
        }
        Mark::Dash => {
            painter.hline(rect.left()..=rect.right(), rect.center().y, stroke);
        }
    }
}
