//! The few pieces the whole interface is built from.
//!
//! Kept small and shared so density stays consistent. A panel drifts into noise when every
//! screen invents its own way of showing a status, so there is one mark, one chip, one card,
//! one section head and one rule, and every tab uses them.
//!
//! **A state is never only a colour.** Every state carries a word and a distinct shape as well,
//! because a screen whose meaning lives in the hue is a screen that stops working for somebody
//! who cannot separate amber from green, and because a shape survives a photograph of a monitor
//! where a hue does not.

use eframe::egui::{Align, Color32, Layout, Response, RichText, Sense, Ui, Vec2, vec2};

use crate::model::{AgentStatus, Diagnostic, Health, Kind, Link};
use crate::theme;

mod card;
mod mark;

pub use card::{Card, Tone, card, connector, fitted_card, sized_card, spine};
pub use mark::{Mark, mark};

#[cfg(test)]
mod tests;

/// The colour a status is drawn in.
pub fn status_color(s: AgentStatus) -> Color32 {
    match s {
        AgentStatus::Working => theme::ACCENT,
        AgentStatus::AwaitingReview => theme::COLD,
        AgentStatus::Blocked => theme::BAD,
        AgentStatus::Idle => theme::FAINT,
        AgentStatus::Unknown => theme::UNKNOWN,
    }
}

/// The shape a status is drawn as, so the state survives without the colour.
pub fn status_mark(s: AgentStatus) -> Mark {
    match s {
        AgentStatus::Working => Mark::Filled,
        AgentStatus::AwaitingReview => Mark::Half,
        AgentStatus::Blocked => Mark::Barred,
        AgentStatus::Idle => Mark::Hollow,
        AgentStatus::Unknown => Mark::Dash,
    }
}

/// The word for a health, since the canonical type is data and does not carry screen words.
pub fn health_label(h: Health) -> &'static str {
    match h {
        Health::Healthy => "HEALTHY",
        Health::Degraded => "DEGRADED",
        Health::Blocked => "BLOCKED",
        Health::Failed => "FAILED",
        Health::Unknown => "UNKNOWN",
    }
}

/// Whether a health should pull the eye.
///
/// Unknown deliberately does not. It is a gap in what was measured rather than a fault, and
/// treating every unmeasured thing as an alarm trains somebody to ignore the screen.
pub fn wants_attention(h: Health) -> bool {
    matches!(h, Health::Failed | Health::Blocked | Health::Degraded)
}

pub fn health_color(h: Health) -> Color32 {
    match h {
        Health::Healthy => theme::GOOD,
        Health::Degraded => theme::WARN,
        Health::Blocked => theme::WARN,
        Health::Failed => theme::BAD,
        Health::Unknown => theme::UNKNOWN,
    }
}

/// The shape a health is drawn as. Five healths, five different shapes, so the two that share
/// a colour are still told apart without reading the word.
pub fn health_mark(h: Health) -> Mark {
    match h {
        Health::Healthy => Mark::Filled,
        Health::Degraded => Mark::Half,
        Health::Blocked => Mark::Barred,
        Health::Failed => Mark::Cross,
        Health::Unknown => Mark::Dash,
    }
}

/// A small square that carries a state colour, kept for the places that want a bare dot.
pub fn pip(ui: &mut Ui, color: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(9.0), Sense::hover());
    mark(
        ui.painter(),
        rect,
        if filled { Mark::Filled } else { Mark::Hollow },
        color,
    );
}

/// A state, drawn the way this panel always draws a state: shape, then word, in a bordered
/// pill so it reads as a token rather than as a stray coloured word in a sentence.
pub fn state_chip(ui: &mut Ui, shape: Mark, word: &str, color: Color32) -> Response {
    let font = theme::label();
    // Laid out inside the room there actually is. `layout_no_wrap` gave the chip whatever width
    // the word wanted, so a long one ran out of the rail and was reported as cut off by it. The
    // rail is narrow on purpose and a chip has to live within it.
    let room = (ui.available_width() - 30.0).max(40.0);
    let galley = ui
        .painter()
        .layout(word.to_string(), font.clone(), color, room);
    let height = (galley.size().y + 8.0).max(22.0);
    let width = galley.size().x + 30.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());

    ui.painter()
        .rect_filled(rect, theme::CORNER, theme::VOID.gamma_multiply(0.9));
    ui.painter().rect_stroke(
        rect,
        theme::CORNER,
        theme::edge(color.linear_multiply(0.55)),
    );

    let box_side = 9.0;
    let box_rect = eframe::egui::Rect::from_center_size(
        eframe::egui::pos2(rect.left() + 13.0, rect.center().y),
        Vec2::splat(box_side),
    );
    mark(ui.painter(), box_rect, shape, color);
    ui.painter().galley(
        eframe::egui::pos2(rect.left() + 23.0, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
    response
}

/// A dim all caps label, spaced rather than shrunk.
pub fn small(ui: &mut Ui, text: &str) {
    ui.label(theme::spaced(text).font(theme::label()).color(theme::FAINT));
}

/// A section heading with a rule running to the right of it, which is what separates one
/// group of cards from the next without drawing a box around the group as well.
pub fn section(ui: &mut Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(theme::spaced(title).font(theme::label()).color(theme::DIM));
        let y = ui.cursor().center().y;
        let x0 = ui.cursor().left() + 8.0;
        let x1 = ui.max_rect().right();
        if x1 > x0 {
            ui.painter().hline(x0..=x1, y, theme::hairline());
        }
    });
    ui.add_space(6.0);
}

/// A section heading with a count on the right, for a group whose size is worth knowing.
pub fn section_count(ui: &mut Ui, title: &str, count: usize, color: Color32) {
    ui.horizontal(|ui| {
        ui.label(theme::spaced(title).font(theme::label()).color(theme::DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(count.to_string())
                    .font(theme::label())
                    .color(color),
            );
        });
    });
    ui.add_space(6.0);
}

/// A full width hairline.
pub fn rule(ui: &mut Ui) {
    let rect = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter()
        .hline(rect.left()..=rect.right(), y, theme::hairline());
    ui.add_space(1.0);
}

/// A key and its value on one line, with the value right aligned.
///
/// `None` draws the words "not known" in the unknown colour rather than an empty space, so a
/// missing figure is visibly missing rather than looking like a rendering fault.
pub fn field(ui: &mut Ui, key: &str, value: Option<&str>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).font(theme::label()).color(theme::FAINT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match value {
            Some(v) => {
                ui.label(RichText::new(v).font(theme::body()).color(theme::TEXT));
            }
            None => {
                ui.label(
                    RichText::new("not known")
                        .font(theme::label())
                        .color(theme::UNKNOWN),
                );
            }
        });
    });
}

/// A row that can be picked, drawn as a band. Used by the rail, where a card inside a card
/// would be one surface too many.
pub fn row(
    ui: &mut Ui,
    height: f32,
    selected: bool,
    lit: bool,
    add: impl FnOnce(&mut Ui),
) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    let hovered = response.hovered();
    let fill = if selected {
        theme::HOVER
    } else if hovered {
        theme::RAISED
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, theme::CORNER, fill);
    }
    if selected || lit {
        let bar = eframe::egui::Rect::from_min_size(rect.min, vec2(3.0, rect.height()));
        let color = if selected {
            theme::ACCENT
        } else {
            theme::ACCENT_DIM
        };
        ui.painter().rect_filled(bar, 0.0, color);
    }

    let inner = rect.shrink2(vec2(12.0, 5.0));
    let mut child = ui.new_child(
        eframe::egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    add(&mut child);
    response
}

/// The link indicator, which is on screen at all times.
///
/// Deliberately the only thing in the whole interface that is allowed to be alarming, because
/// a panel that looks live while disconnected makes every other number on it a lie.
pub fn link_badge(ui: &mut Ui, link: &Link) {
    let (color, shape) = match link {
        Link::Live => (theme::GOOD, Mark::Filled),
        Link::Connecting { .. } => (theme::WARN, Mark::Half),
        Link::Disconnected { .. } => (theme::BAD, Mark::Cross),
    };
    state_chip(ui, shape, &link.label(), color);
}

/// How long ago, in the shortest form that is still true.
pub fn ago(now: u64, then: u64) -> String {
    let d = now.saturating_sub(then);
    match d {
        0..=1 => "now".into(),
        2..=59 => format!("{d}s ago"),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        _ => format!("{}d ago", d / 86_400),
    }
}

/// How a reading's freshness is described, or nothing when freshness is not the point.
///
/// Event driven state gets no age at all. It is true until something changes it, and a clock
/// beside it would say it decays.
pub fn freshness(d: &Diagnostic, now: u64) -> Option<String> {
    match d.kind {
        Kind::EventDriven => None,
        Kind::Sampled => Some(match d.measured_at {
            Some(at) => format!("sampled {}", ago(now, at)),
            None => "never sampled".into(),
        }),
    }
}

/// A name as a person would write it: JJ stays shouted, everything else gets one capital.
///
/// The org table stores lowercase identifiers because they are used as folder names and in the
/// record. That is right for a key and wrong for a line somebody reads, where "nora" next to
/// "JJ" looks like a mistake rather than a distinction.
pub fn proper(name: &str) -> String {
    if name.eq_ignore_ascii_case("jj") {
        return "JJ".to_string();
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod proper_tests {
    use super::proper;

    #[test]
    fn a_name_reads_the_way_a_person_writes_it() {
        assert_eq!(proper("nora"), "Nora");
        assert_eq!(proper("carl"), "Carl");
        assert_eq!(proper("jj"), "JJ", "not Jj");
        assert_eq!(proper(""), "");
    }
}
