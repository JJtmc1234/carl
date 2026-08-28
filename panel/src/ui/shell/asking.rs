//! The band that appears when Carl is waiting on an answer.
//!
//! A held tool call is the one thing on this screen with a clock running against it. A process
//! is stopped, and if nobody answers it is refused and Carl carries on without having done the
//! thing. So this is a full width band above every tab rather than a row inside one, for the
//! same reason the not live warning is: a person must not have to be on the right tab to find
//! out.
//!
//! **The question does not disappear when the button is clicked.** It goes when the backend
//! confirms. A row that vanished optimistically and then failed to send would leave JJ certain
//! he had answered something that is still sitting there with the clock running.

use eframe::egui::{Context, RichText, TopBottomPanel};

use crate::app::App;
use crate::model::Permission;
use crate::theme;

/// How tall one question is.
const ROW: f32 = 46.0;

/// How many are drawn before the rest become a count.
///
/// More than this is not a screen anybody reads, it is a screen anybody dismisses. The backend
/// caps what can be outstanding anyway, so this is about legibility rather than about bounds.
const SHOWN: usize = 3;

pub fn draw(app: &mut App, ctx: &Context) {
    let waiting: Vec<Permission> = app.permissions().to_vec();
    // Answered in the last few seconds, so a press is confirmed where it was made. The band
    // stays up for those even when nothing is waiting, otherwise the only sign a click worked
    // is a row disappearing, and the army asks often enough that another usually replaces it in
    // the same second.
    let settled: Vec<(String, bool)> = app
        .just_settled
        .iter()
        .map(|(tool, ok, _)| (tool.clone(), *ok))
        .collect();

    if waiting.is_empty() && settled.is_empty() {
        return;
    }

    let rows = waiting.len().min(SHOWN);
    let extra = usize::from(waiting.len() > SHOWN);
    let height = ROW * (rows + extra) as f32 + settled.len() as f32 * 22.0 + 10.0;

    // An answer chosen while drawing, applied after, because `app` is borrowed for the frame.
    let mut answered: Option<(String, bool)> = None;

    TopBottomPanel::top("asking")
        .exact_height(height)
        .frame(
            eframe::egui::Frame::none()
                .fill(theme::ACCENT.linear_multiply(0.14))
                .stroke(theme::edge(theme::ACCENT))
                .inner_margin(eframe::egui::Margin::symmetric(16.0, 5.0)),
        )
        .show(ctx, |ui| {
            for (tool, ok) in &settled {
                ui.label(
                    RichText::new(format!(
                        "{} {tool}",
                        if *ok { "ALLOWED" } else { "REFUSED" }
                    ))
                    .font(theme::label())
                    .color(if *ok { theme::GOOD } else { theme::BAD }),
                );
            }
            for request in waiting.iter().take(SHOWN) {
                if let Some(verdict) = question(ui, request) {
                    answered = Some((request.id.clone(), verdict));
                }
            }
            if waiting.len() > SHOWN {
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("and {} more waiting", waiting.len() - SHOWN))
                        .font(theme::label())
                        .color(theme::DIM),
                );
            }
        });

    if let Some((id, allow)) = answered {
        app.answer_permission(&id, allow);
    }
}

/// One question, and the two buttons. `Some(true)` for allow, `Some(false)` for deny.
fn question(ui: &mut eframe::egui::Ui, request: &Permission) -> Option<bool> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.set_height(ROW - 6.0);
        crate::ui::widgets::state_chip(
            ui,
            // Held up by something, which is exactly what a stopped tool call is.
            crate::ui::widgets::Mark::Barred,
            "ASKING",
            theme::ACCENT,
        );
        ui.add_space(10.0);

        ui.vertical(|ui| {
            ui.label(
                RichText::new(&request.tool)
                    .font(theme::label())
                    .color(theme::TEXT),
            );
            // The command or the path, which is the part worth reading before deciding. Cut to
            // one line, because a wrapped shell command would push the buttons off the band.
            ui.label(
                RichText::new(one_line(&request.detail))
                    .font(theme::prose())
                    .color(theme::DIM),
            );
        });

        // Right aligned, so the buttons are in the same place whatever the command's length.
        ui.with_layout(
            eframe::egui::Layout::right_to_left(eframe::egui::Align::Center),
            |ui| {
                // Deny is nearest the edge, which is where a thumb lands, and it is the
                // outcome that happens anyway if nobody touches anything.
                if ui.button(RichText::new("Deny").color(theme::BAD)).clicked() {
                    chosen = Some(false);
                }
                ui.add_space(6.0);
                if ui
                    .button(RichText::new("Allow").color(theme::GOOD))
                    .clicked()
                {
                    chosen = Some(true);
                }
                ui.add_space(12.0);
                ui.label(
                    RichText::new(&request.surface)
                        .font(theme::label())
                        .color(theme::DIM),
                );
            },
        );
    });
    chosen
}

/// One line of it, ellipsised, with newlines turned into spaces.
///
/// A heredoc or a multi line script would otherwise be drawn as several lines and push the
/// buttons out of the band, which is a question with no way to answer it.
pub fn one_line(detail: &str) -> String {
    const MOST: usize = 96;
    let flat: String = detail.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(MOST) {
        Some((at, _)) => format!("{}...", &flat[..at]),
        None => flat,
    }
}
