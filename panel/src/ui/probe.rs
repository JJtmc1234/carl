//! A headless renderer, so the layout can be checked the way somebody looks at it.
//!
//! Unit tests on `App` check the rules and never the drawing, and a panel breaks in the
//! drawing: a label that runs out of its card, two lines painted on top of each other, a column
//! of figures that does not line up, a font that came out too small to read. None of that
//! shows up in a test of the state machine and all of it shows up here, because this actually
//! paints a frame and then reads back every glyph that was put on it, with the rectangle it
//! landed in and the rectangle it was allowed to land in.
//!
//! One row of one galley is one `Painted`. That is finer than one label on purpose: a wrapped
//! sentence has a rectangle per line, and the whole point of this is to catch the second line
//! of one card sitting on top of the first line of the next.

use eframe::egui::{Color32, Context, Pos2, RawInput, Rect, Shape, Vec2};

use crate::app::App;
use crate::theme;

/// One row of text, where it landed, and where it was allowed to land.
#[derive(Debug, Clone)]
pub struct Painted {
    pub text: String,
    pub rect: Rect,
    pub clip: Rect,
    pub size: f32,
    pub color: Color32,
}

impl Painted {
    /// Whether this row was cut off sideways by whatever it was drawn inside.
    ///
    /// Sideways only. A row scrolled past the bottom of a scroll area is clipped vertically and
    /// that is what a scroll area is for. A row cut off at the right hand edge is a card that
    /// could not hold its own contents.
    pub fn cut_off(&self) -> bool {
        let vertically_visible =
            self.rect.top() < self.clip.bottom() && self.rect.bottom() > self.clip.top();
        vertically_visible
            && (self.rect.right() > self.clip.right() + 0.5
                || self.rect.left() < self.clip.left() - 0.5)
    }

    /// Whether anything was actually drawn, as opposed to a row of spaces.
    pub fn is_ink(&self) -> bool {
        !self.text.trim().is_empty() && self.rect.width() > 0.5
    }
}

/// One painted frame.
#[derive(Debug, Clone)]
pub struct Frame {
    pub screen: Rect,
    pub text: Vec<Painted>,
    /// Every filled or stroked rectangle, for measuring how much of the screen is used.
    pub boxes: Vec<Rect>,
    /// Every straight line, which is how the hierarchy connectors are drawn.
    pub lines: Vec<(Pos2, Pos2)>,
}

impl Frame {
    /// Every row whose text contains this, for asking whether something reached the screen.
    pub fn find(&self, needle: &str) -> Vec<&Painted> {
        self.text
            .iter()
            .filter(|p| p.text.contains(needle))
            .collect()
    }

    pub fn says(&self, needle: &str) -> bool {
        !self.find(needle).is_empty()
    }

    /// All the words on the screen, joined, for asserting on a whole screen at once.
    pub fn words(&self) -> String {
        self.text
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Every pair of rows that were painted on top of each other.
    ///
    /// Shrunk by a pixel before testing, because two rows that share an edge are adjacent
    /// rather than overlapping, and antialiasing makes an exact edge test noise.
    pub fn collisions(&self) -> Vec<(&Painted, &Painted)> {
        let ink: Vec<&Painted> = self.text.iter().filter(|p| p.is_ink()).collect();
        let mut out = Vec::new();
        for (i, a) in ink.iter().enumerate() {
            for b in ink.iter().skip(i + 1) {
                if a.rect.shrink(1.0).intersects(b.rect.shrink(1.0)) {
                    out.push((*a, *b));
                }
            }
        }
        out
    }

    /// Every row that was cut off sideways.
    pub fn cut_off(&self) -> Vec<&Painted> {
        self.text
            .iter()
            .filter(|p| p.is_ink() && p.cut_off())
            .collect()
    }

    /// The smallest type anybody was asked to read.
    pub fn smallest(&self) -> f32 {
        self.text
            .iter()
            .filter(|p| p.is_ink())
            .map(|p| p.size)
            .fold(f32::INFINITY, f32::min)
    }

    /// Roughly how much of the screen has something on it.
    ///
    /// A crude coverage figure over a grid, which is enough to catch the failure it is for: a
    /// screen that is mostly empty canvas with a paragraph in one corner. It is not a beauty
    /// score and it is never asserted tightly.
    pub fn coverage(&self, region: Rect) -> f32 {
        let steps = 64;
        let mut hit = 0;
        for ix in 0..steps {
            for iy in 0..steps {
                let at = Pos2::new(
                    region.left() + region.width() * (ix as f32 + 0.5) / steps as f32,
                    region.top() + region.height() * (iy as f32 + 0.5) / steps as f32,
                );
                if self.boxes.iter().any(|r| r.contains(at))
                    || self.text.iter().any(|p| p.rect.contains(at))
                {
                    hit += 1;
                }
            }
        }
        hit as f32 / (steps * steps) as f32
    }
}

/// Paints the whole panel at a size and reads back what landed on it.
///
/// Two frames, not one. egui settles sizes on the frame after the one that discovered them, so
/// a single pass reports the layout of a screen nobody would ever see.
pub fn render(app: &mut App, size: Vec2) -> Frame {
    let ctx = Context::default();
    let screen = Rect::from_min_size(Pos2::ZERO, size);
    let input = || RawInput {
        screen_rect: Some(screen),
        ..Default::default()
    };

    theme::install(&ctx);
    let _ = ctx.run(input(), |ctx| super::draw(app, ctx));
    theme::install(&ctx);
    let out = ctx.run(input(), |ctx| super::draw(app, ctx));

    let mut frame = Frame {
        screen,
        text: Vec::new(),
        boxes: Vec::new(),
        lines: Vec::new(),
    };
    for clipped in &out.shapes {
        gather(clipped.clip_rect, &clipped.shape, &mut frame);
    }
    frame
}

fn gather(clip: Rect, shape: &Shape, into: &mut Frame) {
    match shape {
        Shape::Vec(many) => {
            for one in many {
                gather(clip, one, into);
            }
        }
        Shape::Text(text) => {
            let offset = text.pos.to_vec2();
            let default = text
                .galley
                .job
                .sections
                .first()
                .map(|s| s.format.font_id.size);
            for row in &text.galley.rows {
                let words: String = row.glyphs.iter().map(|g| g.chr).collect();
                let size = row
                    .glyphs
                    .first()
                    .map(|g| g.font_height)
                    .or(default)
                    .unwrap_or(0.0);
                into.text.push(Painted {
                    text: words,
                    rect: row.rect.translate(offset),
                    clip,
                    size,
                    color: text.fallback_color,
                });
            }
        }
        Shape::Rect(rect) => into.boxes.push(rect.rect),
        Shape::LineSegment { points, .. } => {
            into.lines.push((points[0], points[1]));
            into.boxes.push(Rect::from_two_pos(points[0], points[1]));
        }
        _ => {}
    }
}

/// The two sizes every screen is checked at.
///
/// The big one is the panel JJ actually runs it on. The small one is deliberately awkward: a
/// laptop window is where a three column layout finds out whether its columns can hold a
/// sentence.
pub const BIG: Vec2 = Vec2::new(3840.0, 2400.0);
pub const SMALL: Vec2 = Vec2::new(1280.0, 800.0);

/// Everything a clipped shape carried, for a failure message somebody can act on.
pub fn describe(rows: &[&Painted]) -> String {
    rows.iter()
        .map(|p| format!("{:?} at {:?} clipped to {:?}", p.text, p.rect, p.clip))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Turns a list of collisions into something readable in a test failure.
pub fn describe_pairs(pairs: &[(&Painted, &Painted)]) -> String {
    pairs
        .iter()
        .take(12)
        .map(|(a, b)| {
            format!(
                "{:?} {:?} overlaps {:?} {:?}",
                a.text, a.rect, b.text, b.rect
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Convenience for the screens: paint one tab and read it back.
pub fn tab(app: &mut App, tab: crate::app::Tab, size: Vec2) -> Frame {
    app.select_tab(tab);
    render(app, size)
}

mod tests;
