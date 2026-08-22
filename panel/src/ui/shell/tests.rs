//! What the frame has to keep being true about itself.

use super::*;
use crate::model::{AgentStatus, Health, Link};
use crate::source::MockPanelDataSource;
use crate::ui::vitals;

fn app() -> App {
    App::new(Box::new(MockPanelDataSource::new()))
}

/// The rail counts what needs somebody, which is how a tab you are not looking at tells
/// you to look at it.
#[test]
fn the_rail_counts_what_needs_attention() {
    let mut a = app();
    let v = vitals::read(&a.snapshot);
    assert_eq!(rail::wants_attention(&a, Tab::Agents, &v), 0);

    a.snapshot.agents[2].status = AgentStatus::Blocked;
    let v = vitals::read(&a.snapshot);
    assert_eq!(rail::wants_attention(&a, Tab::Agents, &v), 1);

    // Unknown is a gap rather than a fault, so it must not be counted as one.
    a.snapshot.agents[3].status = AgentStatus::Unknown;
    let v = vitals::read(&a.snapshot);
    assert_eq!(rail::wants_attention(&a, Tab::Agents, &v), 1);

    let degraded = a
        .snapshot
        .diagnostics
        .iter()
        .filter(|d| crate::ui::widgets::wants_attention(d.health))
        .count();
    assert_eq!(rail::wants_attention(&a, Tab::Diagnostics, &v), degraded);
    assert!(
        a.snapshot
            .diagnostics
            .iter()
            .any(|d| d.health == Health::Unknown),
        "the fixture must include an unmeasured component"
    );
}

/// The overview badge is the sum of everything that wants a person, so the front page is
/// never quieter than the screens behind it.
#[test]
fn the_overview_badge_is_never_smaller_than_the_screens_it_summarises() {
    let mut a = app();
    a.snapshot.agents[4].status = AgentStatus::Blocked;
    let v = vitals::read(&a.snapshot);

    let overview = rail::wants_attention(&a, Tab::Overview, &v);
    for tab in [Tab::Carl, Tab::Agents, Tab::Projects] {
        assert!(
            overview >= rail::wants_attention(&a, tab, &v),
            "{tab:?} has something the overview does not count"
        );
    }
}

/// The moment the link goes, the panel has to say that what is on screen is old.
#[test]
fn losing_the_link_puts_a_warning_across_the_top() {
    let mut a = app();
    assert_eq!(stale_warning(&a), None, "nothing to say while live");

    a.link = Link::Disconnected {
        why: "backend closed the connection".into(),
    };
    let w = stale_warning(&a).expect("a warning");
    assert!(w.contains("before the link dropped"), "{w}");

    a.link = Link::Connecting { attempt: 2 };
    let w = stale_warning(&a).expect("a warning");
    assert!(w.contains("reconnecting 2"), "{w}");
}

/// Two columns must add back up to the space they were given, or one of them silently
/// overflows the other and the screen has a seam down the middle.
#[test]
fn splitting_into_columns_never_loses_or_invents_width() {
    let ctx = eframe::egui::Context::default();
    let _ = ctx.run(Default::default(), |ctx| {
        CentralPanel::default().show(ctx, |ui| {
            let total = ui.available_width();
            for fraction in [0.34_f32, 0.46, 0.5, 0.66] {
                let (left, right) = columns_for(ui, fraction);
                assert!(
                    left > 0.0 && right > 0.0,
                    "a column with no width at {fraction}"
                );
                assert!(
                    left + right <= total,
                    "the columns are wider than the space at {fraction}"
                );
                assert!(
                    total - (left + right) < theme::GAP + 6.0,
                    "more than a gap went missing at {fraction}"
                );
            }
        });
    });
}

/// The panel heartbeat says how long the window has been up and nothing else, so its wording
/// must never grow into a claim about the army.
#[test]
fn the_uptime_reads_short_and_true() {
    assert_eq!(strip::uptime(0), "0s");
    assert_eq!(strip::uptime(59), "59s");
    assert_eq!(strip::uptime(60), "1m");
    assert_eq!(strip::uptime(7200), "2h");
}
