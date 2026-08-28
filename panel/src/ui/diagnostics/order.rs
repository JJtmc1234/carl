//! Which board a reading belongs on and where it sits, worked out without a window.

use crate::model::{Diagnostic, Health, board_of};

/// The two boards, in the order they are read. The army first, because a machine that is fine
/// while the organisation has stopped is not good news.
pub const BOARDS: [(&str, &str); 2] = [("army", "ARMY"), ("system", "SYSTEM")];

/// One board's rows, worst first and then by name so the order is stable frame to frame.
pub fn sorted<'a>(all: &'a [Diagnostic], group: &str) -> Vec<&'a Diagnostic> {
    let mut rows: Vec<&Diagnostic> = all.iter().filter(|d| board_of(d) == group).collect();
    rows.sort_by(|a, b| {
        worst_first(a.health)
            .cmp(&worst_first(b.health))
            .then(a.component.cmp(&b.component))
    });
    rows
}

/// The order a board is read in. Worst first, and unknown above healthy because a gap is worth
/// noticing before something that is fine.
pub fn worst_first(h: Health) -> u8 {
    match h {
        Health::Failed => 0,
        Health::Blocked => 1,
        Health::Degraded => 2,
        Health::Unknown => 3,
        Health::Healthy => 4,
    }
}

/// The worst health on a board, for the summary over it. `None` when the board is empty, which
/// is a different thing from a board where everything is fine.
pub fn worst_on(all: &[Diagnostic], group: &str) -> Option<Health> {
    sorted(all, group).first().map(|d| d.health)
}

/// How many rows a board has in each health, in the order they are shown.
pub fn tally(all: &[Diagnostic], group: &str) -> [(Health, usize); 5] {
    let mut counts = [
        (Health::Failed, 0),
        (Health::Blocked, 0),
        (Health::Degraded, 0),
        (Health::Unknown, 0),
        (Health::Healthy, 0),
    ];
    for d in all.iter().filter(|d| board_of(d) == group) {
        for slot in counts.iter_mut() {
            if slot.0 == d.health {
                slot.1 += 1;
            }
        }
    }
    counts
}

/// The short name of a component, without the board prefix it is already filed under.
///
/// `system.disk:/` on the system board is just `disk:/`. Repeating the prefix on every row of a
/// board headed with it costs seven characters of every name and says nothing.
pub fn short_name(component: &str) -> &str {
    match component.split_once('.') {
        Some((head, rest)) if head == "army" || head == "system" => rest,
        _ => component,
    }
}

#[cfg(test)]
mod tests;
