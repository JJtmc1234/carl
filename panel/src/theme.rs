//! The visual language, in one place, so there is only ever one of it.
//!
//! The brief is a command centre for a live organisation, which is a different thing from a
//! monitoring tool. A monitor is read by somebody who already knows what they are looking for.
//! A command centre has to tell somebody who has just walked up to it what the state of the
//! world is, and it has to do that at a glance, from across a desk.
//!
//! **One accent, used sparingly.** A cold amber against a near black. Accent means live, or
//! selected, or needs you. If everything glowed, glowing would mean nothing, so most of the
//! interface is grey on black and the eye goes to the few things that are not.
//!
//! **Colour carries state and nothing else.** Never decoration, never a category. And colour
//! is never the only carrier: every state also has a word and a mark, so the screen still
//! works for somebody who cannot tell amber from green.
//!
//! **Depth by value and by edge.** Four background steps do the layering, and cards sit on
//! them with a real one pixel border. The earlier rule of no boxes and no connector lines is
//! cancelled: at this size an unbordered list of rows reads as terminal output, which is the
//! defect this redesign exists to fix. No gradients, no glass, no bevels.
//!
//! **Type does the hierarchy, and it is not small.** The old scale topped out at 20px, which
//! on a 3840x2400 panel is why the thing read as a debug tool. Six roles now, from 13 to 34,
//! and the family is chosen by what the text is rather than by habit: a name, an identifier,
//! a figure or a state word is monospace because it is data and wants to line up. A sentence
//! is proportional because it is prose and wants to be read.

use eframe::egui::{
    Color32, FontFamily, FontId, Painter, Rect, RichText, Rounding, Stroke, TextStyle,
};

/// The background ramp, darkest first. Depth comes from these and from the card edges.
pub const VOID: Color32 = Color32::from_rgb(6, 8, 11);
pub const PANEL: Color32 = Color32::from_rgb(11, 14, 19);
pub const RAISED: Color32 = Color32::from_rgb(17, 21, 28);
pub const HOVER: Color32 = Color32::from_rgb(24, 30, 39);

/// Hairlines and card edges.
pub const RULE: Color32 = Color32::from_rgb(30, 37, 48);
pub const RULE_BRIGHT: Color32 = Color32::from_rgb(48, 58, 74);

/// Text, brightest first. Three steps and no more.
pub const TEXT: Color32 = Color32::from_rgb(214, 222, 233);
pub const DIM: Color32 = Color32::from_rgb(138, 150, 166);
pub const FAINT: Color32 = Color32::from_rgb(88, 99, 114);

/// The one accent. Live, selected, needs you.
pub const ACCENT: Color32 = Color32::from_rgb(255, 176, 63);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(122, 84, 30);

/// State colours. These are the only other hues on the screen.
pub const GOOD: Color32 = Color32::from_rgb(94, 196, 140);
pub const WARN: Color32 = Color32::from_rgb(226, 165, 62);
pub const BAD: Color32 = Color32::from_rgb(232, 92, 84);
pub const COLD: Color32 = Color32::from_rgb(96, 158, 214);
/// Unknown is deliberately colourless. It is a gap, not a state to act on.
pub const UNKNOWN: Color32 = Color32::from_rgb(96, 106, 122);

/// JJ acting directly. Used nowhere except intervention, so it never reads as ordinary.
pub const INTERVENE: Color32 = Color32::from_rgb(214, 96, 168);

pub const CORNER: Rounding = Rounding::same(4.0_f32);
/// Cards are rounded a touch more than controls, so a surface reads as a surface.
pub const CARD_CORNER: Rounding = Rounding::same(6.0_f32);

/// The gap between cards, and the padding inside one. Two numbers, used everywhere, so
/// density is a decision made once rather than at three hundred call sites.
pub const GAP: f32 = 12.0;
pub const PAD: f32 = 14.0;

pub fn hairline() -> Stroke {
    Stroke::new(1.0_f32, RULE)
}

pub fn edge(color: Color32) -> Stroke {
    Stroke::new(1.0_f32, color)
}

/// Which family a role belongs to.
///
/// Both were built and looked at before this was settled. Proportional at 15px for running
/// prose fits about a third more words on a line and reads faster, which matters in the
/// conversation and in the long one line descriptions. Monospace won everywhere a column has
/// to line up, which is most of the rest of the panel.
fn prose_family() -> FontFamily {
    FontFamily::Proportional
}

fn data_family() -> FontFamily {
    FontFamily::Monospace
}

/// Small caps labels, keys and state words. The floor, and it is 13 rather than 11.
pub fn label() -> FontId {
    FontId::new(13.0, data_family())
}

/// Names, identifiers, figures, anything that belongs in a column.
pub fn body() -> FontId {
    FontId::new(15.0, data_family())
}

/// Sentences. The same size as `body`, a different family, because it is not data.
pub fn prose() -> FontId {
    FontId::new(15.0, prose_family())
}

/// A card heading, or an agent's name where it is the subject of the card.
pub fn heading() -> FontId {
    FontId::new(19.0, data_family())
}

/// The name of the screen you are on, and the name of the thing in the inspector.
pub fn title() -> FontId {
    FontId::new(24.0, prose_family())
}

/// The one figure per screen that is allowed to be large.
pub fn display() -> FontId {
    FontId::new(34.0, data_family())
}

/// Every role, for the checks that must hold across all of them.
pub fn every_role() -> [FontId; 6] {
    [label(), body(), prose(), heading(), title(), display()]
}

/// How far apart the letters of a small label sit, in pixels.
///
/// Real spacing, not injected characters. This used to put a space between every letter, which
/// in a monospace font costs a whole cell each time, so a six letter label took eleven cells and
/// read as gappy rather than deliberate. Sub character spacing is the thing that was actually
/// wanted, and egui does it properly.
const LETTER_SPACING: f32 = 1.5;

/// Spreads a short label out, which reads as deliberate where a tiny font reads as cramped.
///
/// Returns styled text rather than a string on purpose. The old version changed the string
/// itself, so the spacing ended up in anything that read the label back: copied text came out
/// with gaps in it, and a screen reader was given "C A R L" to say.
pub fn spaced(text: &str) -> RichText {
    RichText::new(text).extra_letter_spacing(LETTER_SPACING)
}

/// A card surface: raised fill, a real edge, generous padding.
pub fn card_frame() -> eframe::egui::Frame {
    eframe::egui::Frame::none()
        .fill(RAISED)
        .stroke(hairline())
        .rounding(CARD_CORNER)
        .inner_margin(eframe::egui::Margin::same(PAD))
}

/// A halo around something that wants somebody, drawn as three fading strokes.
///
/// Restrained on purpose. This is the only glow in the interface and it exists so a blocked
/// agent or a failed component is findable from across the room. Three strokes, each a third
/// of the last, is a hint of light rather than a neon outline.
pub fn glow(painter: &Painter, rect: Rect, color: Color32, rounding: Rounding) {
    for (i, step) in [1.5_f32, 3.5, 6.0].iter().enumerate() {
        let alpha = 70_u8 >> (i as u8 + 1);
        painter.rect_stroke(
            rect.expand(*step),
            rounding,
            Stroke::new(1.0_f32, color.linear_multiply(alpha as f32 / 255.0)),
        );
    }
}

/// Applies the whole language to a context, once, at startup.
pub fn install(ctx: &eframe::egui::Context) {
    use eframe::egui::{ThemePreference, Visuals, style::Selection};

    // The desktop is in light mode and egui follows it by default. That reapplied light visuals
    // underneath the panel and painted every label that does not name its own colour black on
    // near black. This is a dark interface by design rather than by preference, so it stops
    // asking the desktop what it should look like.
    ctx.options_mut(|o| o.theme_preference = ThemePreference::Dark);

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT);
    visuals.panel_fill = VOID;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = VOID;
    visuals.faint_bg_color = RAISED;
    visuals.selection = Selection {
        bg_fill: ACCENT_DIM,
        stroke: Stroke::new(1.0_f32, ACCENT),
    };
    visuals.widgets.noninteractive.bg_stroke = hairline();
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, DIM);
    visuals.widgets.inactive.bg_fill = RAISED;
    visuals.widgets.inactive.weak_bg_fill = RAISED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.bg_stroke = hairline();
    visuals.widgets.inactive.rounding = CORNER;
    visuals.widgets.hovered.bg_fill = HOVER;
    visuals.widgets.hovered.weak_bg_fill = HOVER;
    visuals.widgets.hovered.bg_stroke = edge(RULE_BRIGHT);
    visuals.widgets.hovered.rounding = CORNER;
    visuals.widgets.active.bg_fill = HOVER;
    visuals.widgets.active.weak_bg_fill = HOVER;
    visuals.widgets.active.bg_stroke = edge(ACCENT);
    visuals.widgets.active.rounding = CORNER;
    visuals.window_stroke = hairline();
    // No drop shadows anywhere. Depth is the background ramp and the card edges.
    visuals.popup_shadow = eframe::egui::epaint::Shadow::NONE;
    visuals.window_shadow = eframe::egui::epaint::Shadow::NONE;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Heading, heading()),
        (TextStyle::Body, body()),
        (TextStyle::Monospace, body()),
        (TextStyle::Button, body()),
        (TextStyle::Small, label()),
    ]
    .into();
    style.spacing.item_spacing = eframe::egui::vec2(10.0, 7.0);
    style.spacing.button_padding = eframe::egui::vec2(12.0, 7.0);
    style.spacing.menu_margin = eframe::egui::Margin::same(8.0);
    style.spacing.scroll.bar_width = 10.0;
    ctx.set_style(style);
}

#[cfg(test)]
mod tests;
