//! The top strip: where you are, and the clocks that prove the screen is still moving.
//!
//! Two different clocks sit here on purpose and they are labelled apart. The journal sequence
//! moves when the army does something. The sample age moves when a number was measured again.
//! A panel that ran them together would let a busy machine make a silent army look busy.
//!
//! The sweep on the right is the panel's own heartbeat and says nothing about the backend. It
//! means this window is still redrawing, which is worth knowing when everything else on screen
//! is quiet, and it is captioned with how long this panel has been up so it cannot be read as
//! a claim about the army.

use std::time::Instant;

use eframe::egui::{Align, Id, Layout, Rect, RichText, TopBottomPanel, Ui, pos2, vec2};

use crate::app::App;
use crate::theme;
use crate::ui::widgets;

pub const HEIGHT: f32 = 62.0;
/// How much room the screen name and its caption are given, so neither ever wraps.
const TITLE_WIDTH: f32 = 460.0;
/// One clock, wide enough for its longest value and its key.
const CLOCK_WIDTH: f32 = 118.0;

pub fn draw(app: &mut App, ctx: &eframe::egui::Context) {
    TopBottomPanel::top("strip")
        .exact_height(HEIGHT)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::PANEL)
                .stroke(theme::hairline())
                .inner_margin(eframe::egui::Margin::symmetric(18.0, 10.0)),
        )
        .show(ctx, |ui| {
            // Both halves are placed at rectangles worked out from the panel itself, rather
            // than by letting a cursor walk across the row. Two earlier arrangements put the
            // clocks on top of the title, and the reason is the same both times: a nested
            // right to left group aligns to whatever the row has claimed so far, not to the
            // panel, so its idea of "the right hand edge" depends on what was drawn before it.
            // Absolute rectangles have no such opinion, and the probe checks it.
            let whole = ui.max_rect();
            let title = TITLE_WIDTH.min(whole.width() * 0.5);

            let left = Rect::from_min_size(whole.min, vec2(title, whole.height()));
            ui.allocate_new_ui(
                eframe::egui::UiBuilder::new()
                    .max_rect(left)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.add_space(-2.0);
                    ui.label(
                        theme::spaced(app.tab.label())
                            .font(theme::title())
                            .color(theme::TEXT),
                    );
                    ui.add_space(-6.0);
                    ui.label(
                        RichText::new(app.tab.caption())
                            .font(theme::label())
                            .color(theme::FAINT),
                    );
                },
            );

            let right = Rect::from_min_max(pos2(left.max.x, whole.min.y), whole.max);
            ui.allocate_new_ui(
                eframe::egui::UiBuilder::new()
                    .max_rect(right)
                    .layout(Layout::right_to_left(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    heartbeat(ui);
                    ui.add_space(20.0);
                    clocks(app, ui);
                    ui.add_space(20.0);
                    notices(app, ui);
                },
            );
        });
}

/// The two clocks, side by side, each labelled with what actually moves it.
fn clocks(app: &App, ui: &mut Ui) {
    let sampled = match app.sampled_at {
        Some(at) => widgets::ago(app.snapshot.at.max(at), at),
        None => "never".into(),
    };
    for (key, value, unmeasured) in [
        ("SAMPLED", sampled, app.sampled_at.is_none()),
        ("SEQ", app.last_seq().to_string(), false),
    ] {
        ui.allocate_ui_with_layout(
            vec2(CLOCK_WIDTH, HEIGHT - 20.0),
            Layout::top_down(Align::Min),
            |ui| {
                ui.add_space(2.0);
                ui.label(RichText::new(value).font(theme::body()).color(
                    // Nothing has been sampled yet, so the figure is drawn as the gap it is
                    // rather than in the same ink as a real reading.
                    if unmeasured {
                        theme::UNKNOWN
                    } else {
                        theme::TEXT
                    },
                ));
                ui.add_space(-6.0);
                ui.label(theme::spaced(key).font(theme::label()).color(theme::FAINT));
            },
        );
        ui.add_space(18.0);
    }
}

fn notices(app: &App, ui: &mut Ui) {
    if let Some((text, ok)) = &app.notice {
        ui.label(RichText::new(text).font(theme::prose()).color(if *ok {
            theme::GOOD
        } else {
            theme::BAD
        }));
        ui.add_space(12.0);
    }
    if let Some(at) = app.resynced_at
        && at.elapsed().as_secs() < 6
    {
        widgets::state_chip(ui, widgets::Mark::Filled, "RESYNCHRONISED", theme::GOOD);
    }
}

/// A sweep across twelve ticks, driven by the frame clock.
///
/// Nothing about it is data. It is the panel saying it is still drawing, which on an idle
/// screen is the difference between quiet and frozen, and the caption underneath says exactly
/// what it is timing so it can never be mistaken for army activity.
fn heartbeat(ui: &mut Ui) {
    let started =
        ui.data_mut(|d| *d.get_temp_mut_or_insert_with(Id::new("panel-began"), Instant::now));
    let up = started.elapsed().as_secs();
    let time = ui.input(|i| i.time);

    ui.vertical(|ui| {
        ui.add_space(4.0);
        let ticks = 12;
        let (rect, _) =
            ui.allocate_exact_size(vec2(ticks as f32 * 6.0, 14.0), eframe::egui::Sense::hover());
        let head = (time * 6.0) as usize % ticks;
        for i in 0..ticks {
            let behind = (ticks + head - i) % ticks;
            let strength = match behind {
                0 => 1.0_f32,
                1 => 0.55,
                2 => 0.3,
                _ => 0.14,
            };
            let x = rect.left() + i as f32 * 6.0;
            let bar = Rect::from_min_max(pos2(x, rect.top()), pos2(x + 3.0, rect.bottom()));
            ui.painter()
                .rect_filled(bar, 0.0, theme::ACCENT.linear_multiply(strength));
        }
        ui.add_space(-4.0);
        ui.label(
            theme::spaced(&format!("PANEL UP {}", uptime(up)))
                .font(theme::label())
                .color(theme::FAINT),
        );
    });
}

/// How long this window has been open, in the shortest form that is still true.
pub fn uptime(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s"),
        60..=3599 => format!("{}m", secs / 60),
        _ => format!("{}h", secs / 3600),
    }
}
