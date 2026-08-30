//! What Carl is doing while he is not yet answering.
//!
//! Two things share this file because they answer the same question. Between asking and the
//! first word of the reply there used to be a placeholder and nothing else, so a Carl reading
//! forty files and a Carl that had wedged looked identical for as long as it took. The tool
//! list says what he is touching. The reasoning says why.
//!
//! Both are drawn above the answer and in a quieter colour than it, because neither is the
//! answer and the reply must stay the thing your eye lands on.

use eframe::egui::{CollapsingHeader, Label, RichText, Ui};

use crate::model::{ToolCall, Turn};
use crate::theme;

/// How many tool calls are listed before the rest become a count.
///
/// A long turn can make hundreds. Listing them all turns the progress note into the wall of
/// text it exists to be an alternative to, and the useful ones are the recent ones.
const SHOW_TOOLS: usize = 6;

/// The width a tool's detail is cut to. Long enough for a path, short enough for one line.
const DETAIL_WIDTH: usize = 64;

/// Draws the reasoning and the tool list for one turn, if it has either.
///
/// `id` distinguishes the collapsing sections between turns. Two turns sharing one id share
/// their open state, which reads as the panel opening a section you did not touch.
pub(crate) fn draw(ui: &mut Ui, turn: &Turn, id: u64) {
    if turn.doing.is_empty() && turn.thinking.trim().is_empty() && turn.thought_tokens.is_none() {
        return;
    }
    ui.push_id(id, |ui| {
        tools(ui, &turn.doing);
        thinking(ui, &turn.thinking, turn.thought_tokens, turn.streaming);
    });
    ui.add_space(4.0);
}

/// The tools picked up in this turn, most recent last.
fn tools(ui: &mut Ui, calls: &[ToolCall]) {
    if calls.is_empty() {
        return;
    }
    // The count is of everything, not of what is shown, so a turn that made ninety calls says
    // ninety rather than six.
    let hidden = calls.len().saturating_sub(SHOW_TOOLS);
    if hidden > 0 {
        ui.label(
            RichText::new(format!("  {hidden} earlier tool call(s)"))
                .font(theme::label())
                .color(theme::FAINT),
        );
    }
    for call in calls.iter().skip(hidden) {
        ui.add(Label::new(
            RichText::new(line_for(call))
                .font(theme::label())
                .color(theme::DIM),
        ));
    }
}

/// One tool call on one line.
///
/// Kept as a pure function so the shortening can be tested without a screen.
pub(crate) fn line_for(call: &ToolCall) -> String {
    // Whitespace collapsed first. A heredoc or a multi line command would otherwise take the
    // row count with it, and the useful part is the beginning either way.
    let detail: String = call.detail.split_whitespace().collect::<Vec<_>>().join(" ");
    let detail = match detail.char_indices().nth(DETAIL_WIDTH) {
        Some((at, _)) => format!("{}...", &detail[..at]),
        None => detail,
    };
    match detail.is_empty() {
        true => format!("  > {}", call.tool),
        false => format!("  > {} {detail}", call.tool),
    }
}

/// Carl's reasoning, collapsed by default.
///
/// Collapsed because it is longer than the answer and is not addressed to anybody. Open on one
/// click because when an answer is taking too long it is the only thing that says why.
fn thinking(ui: &mut Ui, text: &str, tokens: Option<u32>, streaming: bool) {
    let text = text.trim();

    // The usual case, and the one that mattered. The CLI sends the thinking events with the
    // text redacted and the size attached, so there is nothing to expand and a collapsing
    // header would open onto an empty box. A line saying he is thinking and roughly how much
    // is the whole of what is available, and it is far better than showing nothing at all,
    // which is what a reader saw before and read as a stalled agent.
    if text.is_empty() {
        if let Some(n) = tokens {
            ui.label(
                RichText::new(format!("  thinking, about {n} tokens so far"))
                    .font(theme::label())
                    .color(theme::FAINT),
            );
        }
        return;
    }
    CollapsingHeader::new(
        RichText::new(heading_for(text, streaming))
            .font(theme::label())
            .color(theme::FAINT),
    )
    .id_salt("thinking")
    .default_open(false)
    .show(ui, |ui| {
        ui.add(Label::new(
            RichText::new(text).font(theme::prose()).color(theme::DIM),
        ));
    });
}

/// The one line summary on the collapsed section.
///
/// It carries the tail rather than the head while the answer is still being produced. The head
/// stops changing after a second and a heading that never moves is the thing that made the old
/// placeholder useless.
pub(crate) fn heading_for(text: &str, streaming: bool) -> String {
    const PREVIEW: usize = 60;
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !streaming {
        return format!("REASONING, {} characters", text.len());
    }
    let tail = match flat.char_indices().nth_back(PREVIEW) {
        Some((at, _)) => format!("...{}", &flat[at..]),
        None => flat,
    };
    format!("THINKING  {tail}")
}

#[cfg(test)]
mod tests;
