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

fn asked(a: &mut App, id: &str, tool: &str, detail: &str) {
    a.snapshot.permissions.push(crate::model::Permission {
        id: id.into(),
        tool: tool.into(),
        detail: detail.into(),
        surface: "jj".into(),
        asked_at: 10,
    });
}

/// A process is stopped waiting for this, so it has to be on screen whatever tab JJ is on.
#[test]
fn a_held_tool_call_is_visible_from_every_tab() {
    for tab in [Tab::Overview, Tab::Carl, Tab::Agents, Tab::Diagnostics] {
        let mut a = app();
        asked(&mut a, "q1", "Bash", "cargo test --lib panel");
        a.select_tab(tab);

        let frame = crate::ui::probe::render(&mut a, eframe::egui::vec2(1600.0, 1000.0));
        assert!(frame.says("Bash"), "{tab:?} does not show the tool");
        assert!(
            frame.says("Allow") && frame.says("Deny"),
            "{tab:?} shows the question with no way to answer it"
        );
        assert!(
            frame.says("cargo test"),
            "{tab:?} asks without saying what it would run"
        );
    }
}

/// Nothing is asking, so nothing about asking is on screen.
#[test]
fn with_nothing_waiting_the_band_is_not_there() {
    let mut a = app();
    let frame = crate::ui::probe::render(&mut a, eframe::egui::vec2(1600.0, 1000.0));
    assert!(!frame.says("Allow"), "a band with no question in it");
}

/// A question drawn on top of the strip is a question nobody can read or answer.
#[test]
fn the_band_does_not_land_on_top_of_anything() {
    let mut a = app();
    asked(&mut a, "q1", "Bash", "cargo test --lib panel");
    asked(
        &mut a,
        "q2",
        "Write",
        "/home/jj_tmc/Projects/carl/src/lib.rs",
    );

    for size in [
        eframe::egui::vec2(1280.0, 800.0),
        eframe::egui::vec2(1600.0, 1000.0),
        eframe::egui::vec2(1920.0, 1200.0),
        eframe::egui::vec2(2560.0, 1440.0),
    ] {
        let frame = crate::ui::probe::render(&mut a, size);
        let hits = frame.collisions();
        assert!(
            hits.is_empty(),
            "at {size:?}: {}",
            crate::ui::probe::describe_pairs(&hits)
        );
    }
}

/// A long shell command must not push the buttons out of the band.
#[test]
fn a_long_command_is_cut_rather_than_wrapped() {
    let long = "for f in $(find /home/jj_tmc/Projects -name '*.rs'); do echo checking \
                a very long path $f and then some more words after it; done";
    let cut = asking::one_line(long);
    assert!(cut.len() < long.len(), "it was not shortened");
    assert!(cut.ends_with("..."), "and it says it was cut: {cut}");
    assert!(
        !cut.contains('\n'),
        "a wrapped command pushes the buttons off"
    );

    let mut a = app();
    asked(&mut a, "q1", "Bash", long);
    let frame = crate::ui::probe::render(&mut a, eframe::egui::vec2(1280.0, 800.0));
    assert!(
        frame.says("Allow") && frame.says("Deny"),
        "the buttons went off the band"
    );
}

/// More questions than fit must be counted rather than silently dropped.
#[test]
fn questions_past_the_visible_few_are_counted_and_not_hidden() {
    let mut a = app();
    for n in 0..6 {
        asked(&mut a, &format!("q{n}"), "Bash", "echo hi");
    }
    let frame = crate::ui::probe::render(&mut a, eframe::egui::vec2(1600.0, 1000.0));
    assert!(
        frame.says("more waiting"),
        "six questions and no sign of the ones off screen: {}",
        frame.words()
    );
}
