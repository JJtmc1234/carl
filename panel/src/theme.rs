//! The visual language, in one place, so there is only ever one of it.
//!
//! The brief is science fiction and disciplined, which pull against each other, and the way
//! they are reconciled here is that the decoration budget is spent on very few things.
//!
//! **One accent, used sparingly.** A cold amber against a near black. Accent means live, or
//! selected, or needs you. If everything glowed, glowing would mean nothing, so most of the
//! interface is grey on black and the eye goes to the few things that are not.
//!
//! **Colour carries state and nothing else.** Never decoration, never a category. Health and
//! agent status are the only things that get a hue, so a coloured thing on this screen is
//! always something you might have to act on.
//!
//! **Depth by value, not by shadow.** Four background steps do the layering. No gradients, no
//! glass, no bevels.
//!
//! **Type does the hierarchy.** A tight scale, monospace throughout because almost everything
//! here is an identifier, a figure or a state, and letter spacing on the small labels rather
//! than making them smaller. Nothing is below 11px.

use eframe::egui::{Color32, FontFamily, FontId, Rounding, Stroke, TextStyle};

/// The background ramp, darkest first. Depth comes from these rather than from shadows.
pub const VOID: Color32 = Color32::from_rgb(6, 8, 11);
pub const PANEL: Color32 = Color32::from_rgb(11, 14, 19);
pub const RAISED: Color32 = Color32::from_rgb(17, 21, 28);
pub const HOVER: Color32 = Color32::from_rgb(24, 30, 39);

/// Hairlines. The interface is built from rules rather than boxes with edges.
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

pub const CORNER: Rounding = Rounding::same(3.0_f32);

pub fn hairline() -> Stroke {
    Stroke::new(1.0_f32, RULE)
}

pub fn edge(color: Color32) -> Stroke {
    Stroke::new(1.0_f32, color)
}

/// Text roles. Named for what they are for, so a size is never picked at a call site.
pub fn heading() -> FontId {
    FontId::new(15.0, FontFamily::Monospace)
}

pub fn body() -> FontId {
    FontId::new(13.0, FontFamily::Monospace)
}

/// Small caps style labels. 11px is the floor and letter spacing does the work instead of
/// going smaller.
pub fn label() -> FontId {
    FontId::new(11.0, FontFamily::Monospace)
}

pub fn big() -> FontId {
    FontId::new(20.0, FontFamily::Monospace)
}

/// Spreads a short label out, which reads as deliberate where a tiny font reads as cramped.
pub fn spaced(text: &str) -> String {
    text.chars()
        .flat_map(|c| [c, ' '])
        .collect::<String>()
        .trim_end()
        .to_string()
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
    // No drop shadows anywhere. Depth is the background ramp.
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
    style.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    style.spacing.button_padding = eframe::egui::vec2(9.0, 5.0);
    style.spacing.menu_margin = eframe::egui::Margin::same(6.0);
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing below 11px, whatever the density pressure. A panel somebody squints at is one
    /// they stop using.
    #[test]
    fn no_text_role_is_too_small_to_read() {
        for f in [heading(), body(), label(), big()] {
            assert!(f.size >= 11.0, "{f:?} is too small");
        }
    }

    /// Unknown must not look like a state worth acting on, so it carries no hue.
    #[test]
    fn unknown_is_colourless() {
        let (r, g, b, _) = UNKNOWN.to_tuple();
        let spread = r.max(g).max(b) - r.min(g).min(b);
        assert!(spread < 32, "unknown should be near grey, got {r},{g},{b}");
    }

    /// The accent has to stand off the background hard enough to mean something, since it is
    /// the only thing on screen that says look here.
    #[test]
    fn the_accent_stands_off_the_background() {
        let lum = |c: Color32| {
            let (r, g, b, _) = c.to_tuple();
            0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
        };
        assert!(
            lum(ACCENT) - lum(VOID) > 100.0,
            "the accent is not separated from the background"
        );
        assert!(lum(TEXT) - lum(VOID) > 100.0, "body text is not readable");
        assert!(
            lum(DIM) - lum(VOID) > 40.0,
            "secondary text has faded into the background"
        );
    }

    /// The bug JJ saw. The desktop is in light mode, egui follows the desktop by default, and
    /// every label that does not name its own colour came out black on near black.
    ///
    /// Checked against a real context rather than against the constants, because the constants
    /// were always right. What was wrong was that something put a light theme back over them.
    #[test]
    fn the_installed_theme_is_dark_whatever_the_desktop_prefers() {
        let ctx = eframe::egui::Context::default();
        ctx.options_mut(|o| o.theme_preference = eframe::egui::ThemePreference::Light);

        install(&ctx);

        // The preference is the thing that was actually missing. egui reapplies it on every
        // frame, so setting visuals once and leaving the preference on Light meant the desktop
        // won a fraction of a second later. Asserting only on visuals would have passed against
        // the broken build.
        assert_eq!(
            ctx.options(|o| o.theme_preference),
            eframe::egui::ThemePreference::Dark,
            "the panel must stop following the desktop, not just paint over it once"
        );

        let visuals = ctx.style().visuals.clone();
        assert!(
            visuals.dark_mode,
            "the panel must not follow a light desktop"
        );
        assert_eq!(visuals.override_text_color, Some(TEXT));
        assert_eq!(visuals.panel_fill, VOID);

        // The thing that actually went wrong: text no lighter than its background.
        let lum = |c: Color32| {
            let (r, g, b, _) = c.to_tuple();
            0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
        };
        let text = visuals.override_text_color.expect("text colour");
        assert!(
            lum(text) - lum(visuals.panel_fill) > 100.0,
            "text is not readable against the panel"
        );
        assert!(
            lum(visuals.widgets.inactive.fg_stroke.color) > 120.0,
            "unstyled widget text came out dark"
        );
    }

    /// Installing twice must be the same as installing once, since it now runs every frame.
    #[test]
    fn installing_repeatedly_settles() {
        let ctx = eframe::egui::Context::default();
        install(&ctx);
        let once = ctx.style().visuals.clone();
        install(&ctx);
        install(&ctx);
        assert_eq!(
            ctx.style().visuals.override_text_color,
            once.override_text_color
        );
        assert_eq!(ctx.style().visuals.panel_fill, once.panel_fill);
    }

    /// The bug that cost three wrong diagnoses.
    ///
    /// `eframe` is declared with `default-features = false`, and `default_fonts` is one of the
    /// defaults. Without it egui has no fonts at all: every shape still draws, so the panel
    /// came up with correct colours, borders, status pips and hairlines and not one glyph. It
    /// looked exactly like black text on a black background and was nothing of the sort.
    ///
    /// The earlier check inspected the colour each text shape asked for, which was always
    /// right, and so never noticed there were no glyphs to paint in it. This measures the
    /// glyphs instead.
    #[test]
    fn there_are_fonts_and_they_produce_actual_glyphs() {
        let ctx = eframe::egui::Context::default();
        install(&ctx);
        // A frame, so the font atlas is built.
        let _ = ctx.run(Default::default(), |_| {});

        for font in [heading(), body(), label(), big()] {
            let galley = ctx.fonts(|f| f.layout_no_wrap("CARL".to_string(), font.clone(), TEXT));
            assert!(
                galley.size().x > 1.0,
                "{font:?} laid out no width, which means no font is loaded"
            );
            assert!(galley.size().y > 1.0, "{font:?} laid out no height");
            let glyphs: usize = galley.rows.iter().map(|r| r.glyphs.len()).sum();
            assert_eq!(
                glyphs, 4,
                "{font:?} produced {glyphs} glyphs for four letters"
            );
        }
    }

    /// Both families have to work, because the interface asks for monospace everywhere and a
    /// missing monospace atlas would be invisible in a proportional check.
    #[test]
    fn both_font_families_are_present() {
        use eframe::egui::{FontFamily, FontId};

        let ctx = eframe::egui::Context::default();
        install(&ctx);
        let _ = ctx.run(Default::default(), |_| {});

        for family in [FontFamily::Monospace, FontFamily::Proportional] {
            let galley = ctx.fonts(|f| {
                f.layout_no_wrap(
                    "AGENTS".to_string(),
                    FontId::new(13.0, family.clone()),
                    TEXT,
                )
            });
            assert!(
                galley.size().x > 1.0,
                "{family:?} is not loaded, so anything drawn in it is invisible"
            );
        }
    }

    /// Spacing a label rather than shrinking it is how the small type stays legible.
    #[test]
    fn a_label_is_spread_not_shrunk() {
        assert_eq!(spaced("CARL"), "C A R L");
        assert_eq!(spaced(""), "");
    }
}
