//! What the visual language has to keep being true about itself.

use super::*;
use eframe::egui::Color32;

fn lum(c: Color32) -> f32 {
    let (r, g, b, _) = c.to_tuple();
    0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32
}

/// The floor moved from 11 to 13. Eleven pixels on a 3840 wide panel is the single biggest
/// reason the old screen read as a debug tool rather than as an instrument.
#[test]
fn nothing_is_below_the_thirteen_pixel_floor() {
    for f in every_role() {
        assert!(f.size >= 13.0, "{f:?} is below the floor");
    }
}

/// A scale with two usable steps is not a scale. Six distinct sizes, each meaningfully apart
/// from the next, is what lets a card have a heading and a caption that are telling apart.
#[test]
fn the_scale_has_six_distinct_steps_that_climb() {
    let mut sizes: Vec<f32> = every_role().iter().map(|f| f.size).collect();
    sizes.sort_by(f32::total_cmp);
    sizes.dedup();
    assert_eq!(
        sizes,
        vec![13.0, 15.0, 19.0, 24.0, 34.0],
        "five sizes across six roles, since prose and body share a size and differ by family"
    );
    assert!(
        display().size / label().size > 2.5,
        "the top of the scale has to be far enough from the bottom to build a hierarchy with"
    );
}

/// Data lines up and prose does not have to. Getting this the wrong way round is how a panel
/// ends up with ragged columns of figures and cramped sentences.
#[test]
fn the_family_follows_what_the_text_is() {
    for f in [label(), body(), heading(), display()] {
        assert_eq!(f.family, FontFamily::Monospace, "{f:?} carries data");
    }
    for f in [prose(), title()] {
        assert_eq!(f.family, FontFamily::Proportional, "{f:?} carries words");
    }
    assert_eq!(
        prose().size,
        body().size,
        "prose and body must sit on the same line without one looking like a mistake"
    );
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

/// The redesign adds cards, and a card is only a card if you can see where it ends. The four
/// surfaces have to stay distinguishable and the edge has to stand off the surface it is on.
#[test]
fn the_layering_is_visible_rather_than_theoretical() {
    let ramp = [VOID, PANEL, RAISED, HOVER];
    for pair in ramp.windows(2) {
        assert!(
            lum(pair[1]) - lum(pair[0]) > 2.0,
            "{:?} and {:?} are the same surface",
            pair[0],
            pair[1]
        );
    }
    assert!(
        lum(RULE) - lum(RAISED) > 4.0,
        "a card edge that does not stand off the card is not an edge"
    );
    assert!(lum(RULE_BRIGHT) > lum(RULE), "the two rules are one rule");
}

/// The bug JJ saw. The desktop is in light mode, egui follows the desktop by default, and
/// every label that does not name its own colour came out black on near black.
#[test]
fn the_installed_theme_is_dark_whatever_the_desktop_prefers() {
    let ctx = eframe::egui::Context::default();
    ctx.options_mut(|o| o.theme_preference = eframe::egui::ThemePreference::Light);

    install(&ctx);

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
/// came up with correct colours, borders, status pips and hairlines and not one glyph.
#[test]
fn there_are_fonts_and_they_produce_actual_glyphs() {
    let ctx = eframe::egui::Context::default();
    install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});

    for font in every_role() {
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

/// Both families have to work, because half the scale is in each and a missing atlas on
/// either side would take out half the interface.
#[test]
fn both_font_families_are_present() {
    let ctx = eframe::egui::Context::default();
    install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});

    for family in [FontFamily::Monospace, FontFamily::Proportional] {
        let galley = ctx.fonts(|f| {
            f.layout_no_wrap(
                "AGENTS".to_string(),
                FontId::new(15.0, family.clone()),
                TEXT,
            )
        });
        assert!(
            galley.size().x > 1.0,
            "{family:?} is not loaded, so anything drawn in it is invisible"
        );
    }
}

/// The reason prose is proportional, measured rather than asserted by taste. If this ever
/// stops being true the family choice should be revisited rather than kept out of habit.
#[test]
fn proportional_prose_fits_more_of_a_sentence_on_a_line() {
    let ctx = eframe::egui::Context::default();
    install(&ctx);
    let _ = ctx.run(Default::default(), |_| {});

    let sentence = "Handed to Adrian as a coding objective, and he is routing it to Mason.";
    let width = |font: FontId| {
        ctx.fonts(|f| f.layout_no_wrap(sentence.to_string(), font, TEXT))
            .size()
            .x
    };
    assert!(
        width(prose()) < width(body()) * 0.85,
        "proportional is not buying enough line to be worth a second family"
    );
}

/// Spacing a label rather than shrinking it is how the small type stays legible.
#[test]
fn a_label_is_spread_not_shrunk() {
    assert_eq!(spaced("CARL").text(), "CARL");
    assert_eq!(spaced("").text(), "");
}
