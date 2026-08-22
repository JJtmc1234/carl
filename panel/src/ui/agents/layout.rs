//! Where every card goes, worked out before anything is painted.
//!
//! Two jobs, both pure, both testable without a window.
//!
//! **Which cards there are.** Depth first down the real chain out of `army::org`, skipping
//! whatever sits under a collapsed subtree and recording how many were skipped so the card can
//! say so. A department that has been folded away must never look like a department that is
//! empty.
//!
//! **Where they sit.** Positions are computed for the whole tree in one pass and handed back
//! as rectangles. That is what makes the connectors possible: a line from a parent to a child
//! cannot be drawn while laying out, because at the moment the parent is drawn nothing knows
//! where the child will land.

use std::collections::BTreeSet;

use eframe::egui::{Pos2, Rect, pos2, vec2};

use carl::army::org::Rank;

/// How far in one level of the chain sits.
pub const INDENT: f32 = 36.0;
/// The gap between one card and the next.
pub const GAP: f32 = 10.0;

/// One agent's place in the drawn tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub depth: usize,
    /// How many report directly to this agent.
    pub reports: usize,
    /// How many of this agent's descendants are folded away right now.
    pub hidden: usize,
    pub collapsed: bool,
}

impl Node {
    pub fn can_collapse(&self) -> bool {
        self.reports > 0
    }
}

/// Everybody who reports to this agent, by name, in a stable order.
pub fn reports_of(name: &str) -> Vec<String> {
    let mut below: Vec<&'static carl::army::org::Agent> = carl::army::org::reports_of(name);
    below.sort_by_key(|a| a.name);
    below.into_iter().map(|a| a.name.to_string()).collect()
}

/// The people. Outside the operational army, drawn apart from it, never counted as capacity.
pub fn command_authority() -> Vec<String> {
    carl::army::org::everyone()
        .iter()
        .filter(|a| a.rank == Rank::Human)
        .map(|a| a.name.to_string())
        .collect()
}

/// Where the operational army starts: whoever answers to a person, plus any agent that answers
/// to nobody at all and is not a person themselves.
pub fn army_roots() -> Vec<String> {
    let mut roots: Vec<String> = command_authority()
        .iter()
        .flat_map(|human| reports_of(human))
        .collect();
    for a in carl::army::org::everyone() {
        if a.is_root() && a.rank != Rank::Human {
            roots.push(a.name.to_string());
        }
    }
    roots.sort();
    roots.dedup();
    roots
}

/// The visible tree, depth first, honouring what has been folded away.
pub fn arrange(
    roots: &[String],
    reports: &dyn Fn(&str) -> Vec<String>,
    collapsed: &BTreeSet<String>,
) -> Vec<Node> {
    fn descendants(name: &str, reports: &dyn Fn(&str) -> Vec<String>) -> usize {
        reports(name)
            .iter()
            .map(|child| 1 + descendants(child, reports))
            .sum()
    }

    fn under(
        name: &str,
        depth: usize,
        reports: &dyn Fn(&str) -> Vec<String>,
        collapsed: &BTreeSet<String>,
        out: &mut Vec<Node>,
    ) {
        let below = reports(name);
        let folded = collapsed.contains(name) && !below.is_empty();
        out.push(Node {
            name: name.to_string(),
            depth,
            reports: below.len(),
            hidden: if folded {
                descendants(name, reports)
            } else {
                0
            },
            collapsed: folded,
        });
        if folded {
            return;
        }
        for child in below {
            under(&child, depth + 1, reports, collapsed, out);
        }
    }

    let mut out = Vec::new();
    for root in roots {
        under(root, 0, reports, collapsed, &mut out);
    }
    out
}

/// How tall one card is.
///
/// Not one size for everybody. A card big enough to carry a blocker sentence would be a waste
/// of a screen for twenty idle workers, and a card small enough for twenty workers cannot show
/// why one of them has stopped. So the compact size is the default, a lead gets a little more
/// because it carries a department as well as a name, and only an agent that has actually
/// stopped is given the room to say why.
pub fn card_height(rank: Option<Rank>, attention: bool) -> f32 {
    // Worked out from the type scale rather than typed in. These were four constants that fitted
    // the fonts they were written against, and when the scale grew they quietly started cutting
    // the last line off: Nora's card ate "no activity recorded". A number derived from the fonts
    // cannot fall behind them.
    //
    // A card is a name row, a role row, and an activity row, with a blocker given a second line
    // because a reason for stopping is a sentence and not a word.
    let name = crate::theme::heading().size;
    let role = crate::theme::label().size;
    let line = crate::theme::prose().size;

    // Leading, then the padding the card itself adds top and bottom, then a little air so a
    // descender never sits on the border.
    let leading = 1.45;
    let padding = 20.0 + 8.0;

    let rows = (name + role) * leading + line * leading + padding;
    let rows = match rank {
        Some(Rank::Chief) => rows + 8.0,
        Some(Rank::Lead) => rows + 4.0,
        _ => rows,
    };
    if attention {
        // The blocker sentence wraps to a second line more often than not.
        rows + line * leading + 6.0
    } else {
        rows
    }
}

/// A card, placed.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// Index into the node list this came from.
    pub at: usize,
    pub rect: Rect,
    /// Index of the card this one reports to, when it is on screen.
    pub parent: Option<usize>,
}

/// Where the spine of a child at this depth runs, in the parent's left margin.
pub fn spine_x(left: f32, depth: usize) -> f32 {
    left + (depth as f32 - 1.0) * INDENT + INDENT / 2.0
}

/// Lays the whole tree out inside a region, top down.
///
/// Returns the placements and the total height, so a scroll area can be told how tall its
/// contents are before a single card is drawn.
pub fn place(nodes: &[Node], heights: &[f32], top_left: Pos2, width: f32) -> (Vec<Placed>, f32) {
    debug_assert_eq!(nodes.len(), heights.len());
    let mut out: Vec<Placed> = Vec::with_capacity(nodes.len());
    // The most recent card seen at each depth, so a child can find its parent without a second
    // pass over the tree.
    let mut latest: Vec<usize> = Vec::new();
    let mut y = top_left.y;

    for (i, node) in nodes.iter().enumerate() {
        let left = top_left.x + node.depth as f32 * INDENT;
        let rect = Rect::from_min_size(
            pos2(left, y),
            vec2((width - (left - top_left.x)).max(120.0), heights[i]),
        );
        latest.truncate(node.depth);
        let parent = if node.depth == 0 {
            None
        } else {
            latest.get(node.depth - 1).copied()
        };
        out.push(Placed {
            at: i,
            rect,
            parent,
        });
        latest.push(i);
        y += heights[i] + GAP;
    }

    let total = (y - GAP - top_left.y).max(0.0);
    (out, total)
}

#[cfg(test)]
mod tests;
