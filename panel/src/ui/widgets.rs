//! The few pieces the whole interface is built from.
//!
//! Kept small and shared so density stays consistent. A panel drifts into noise when every
//! screen invents its own way of showing a status, so there is one pip, one chip, one section
//! head and one rule, and every tab uses them.

use eframe::egui::{Align, Color32, Layout, Rect, Response, RichText, Sense, Ui, Vec2, vec2};

use crate::model::{AgentStatus, Health, Link, Reading};
use crate::theme;

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

pub fn health_color(h: Health) -> Color32 {
    match h {
        Health::Healthy => theme::GOOD,
        Health::Degraded => theme::WARN,
        Health::Blocked => theme::WARN,
        Health::Failed => theme::BAD,
        Health::Unknown => theme::UNKNOWN,
    }
}

/// A small square that carries a state colour.
///
/// A square rather than a circle, and drawn hollow when nothing is known. An empty outline
/// reads as absence where a filled grey dot reads as a state somebody chose.
pub fn pip(ui: &mut Ui, color: Color32, filled: bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(7.0), Sense::hover());
    if filled {
        ui.painter().rect_filled(rect, 1.0, color);
    } else {
        ui.painter().rect_stroke(rect, 1.0, theme::edge(color));
    }
}

/// A short state word in its own colour.
pub fn chip(ui: &mut Ui, text: &str, color: Color32) {
    ui.label(
        RichText::new(text)
            .font(theme::label())
            .color(color)
            .background_color(theme::RAISED),
    );
}

/// A dim all caps label, spaced rather than shrunk.
pub fn small(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(theme::spaced(text))
            .font(theme::label())
            .color(theme::FAINT),
    );
}

/// A section heading with a rule running to the right of it, which is what gives the layout
/// its structure without drawing boxes around everything.
pub fn section(ui: &mut Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(theme::spaced(title))
                .font(theme::label())
                .color(theme::DIM),
        );
        let y = ui.cursor().center().y;
        let x0 = ui.cursor().left() + 6.0;
        let x1 = ui.max_rect().right();
        if x1 > x0 {
            ui.painter().hline(x0..=x1, y, theme::hairline());
        }
    });
    ui.add_space(4.0);
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
/// `None` draws a dash in the unknown colour rather than an empty space, so a missing figure
/// is visibly missing rather than looking like a rendering fault.
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

/// A row that can be picked, drawn as a band rather than a button.
///
/// Selection is a left bar in the accent plus a lifted background. No border, because a screen
/// of bordered rows is a screen of boxes.
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
    if selected {
        let bar = Rect::from_min_size(rect.min, vec2(2.0, rect.height()));
        ui.painter().rect_filled(bar, 0.0, theme::ACCENT);
    } else if lit {
        // A change that just landed. A left bar again, so it reads in the same language as
        // selection rather than as a new kind of marking.
        let bar = Rect::from_min_size(rect.min, vec2(2.0, rect.height()));
        ui.painter().rect_filled(bar, 0.0, theme::ACCENT_DIM);
    }

    let inner = rect.shrink2(vec2(10.0, 3.0));
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
    let (color, filled) = match link {
        Link::Live => (theme::GOOD, true),
        Link::Connecting { .. } => (theme::WARN, false),
        Link::Disconnected { .. } => (theme::BAD, true),
    };
    ui.horizontal(|ui| {
        pip(ui, color, filled);
        ui.label(
            RichText::new(theme::spaced(&link.label()))
                .font(theme::label())
                .color(color),
        );
    });
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
pub fn freshness(reading: Reading, measured_at: Option<u64>, now: u64) -> Option<String> {
    match reading {
        Reading::EventDriven => None,
        Reading::Sampled => Some(match measured_at {
            Some(at) => format!("sampled {}", ago(now, at)),
            None => "never sampled".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two states nobody has to act on must not carry an alarming colour, and the two that
    /// do must not be the same as each other.
    #[test]
    fn colour_means_state_and_nothing_else() {
        assert_eq!(status_color(AgentStatus::Unknown), theme::UNKNOWN);
        assert_eq!(status_color(AgentStatus::Blocked), theme::BAD);
        assert_ne!(
            status_color(AgentStatus::Working),
            status_color(AgentStatus::Idle),
            "busy and idle must be distinguishable at a glance"
        );
        assert_eq!(health_color(Health::Unknown), theme::UNKNOWN);
        assert_eq!(health_color(Health::Failed), theme::BAD);
    }

    #[test]
    fn ago_says_the_shortest_true_thing() {
        assert_eq!(ago(100, 100), "now");
        assert_eq!(ago(130, 100), "30s ago");
        assert_eq!(ago(1000, 100), "15m ago");
        assert_eq!(ago(100_000, 100), "1d ago");
    }

    /// A state does not decay, so it is not given an age. A sample does, and one that was never
    /// taken says exactly that rather than showing a plausible number.
    #[test]
    fn only_a_sample_is_described_as_fresh_or_stale() {
        assert_eq!(freshness(Reading::EventDriven, Some(10), 100), None);
        assert_eq!(
            freshness(Reading::Sampled, None, 100).as_deref(),
            Some("never sampled")
        );
        assert_eq!(
            freshness(Reading::Sampled, Some(70), 100).as_deref(),
            Some("sampled 30s ago")
        );
    }
}
