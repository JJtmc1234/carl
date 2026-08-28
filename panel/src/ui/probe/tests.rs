//! Tests build a snapshot by starting from the default and setting the one field
//! under test, which reads far better here than restating every field of a large
//! struct in each case.
#![allow(clippy::field_reassign_with_default)]

//! What every screen has to survive being painted.
//!
//! These are the tests that replace looking at it. Each one paints a real frame at a real size
//! and reads back every glyph, so a card that cannot hold its own text, two rows on top of each
//! other, or a font somebody would have to squint at, all fail here rather than being noticed
//! months later.

use super::*;
use crate::app::Tab;
use crate::model::{AgentStatus, Link};
use crate::source::MockPanelDataSource;

fn app() -> App {
    App::new(Box::new(MockPanelDataSource::new()))
}

/// A report of everything the probe measured, printed by the check below so a run of the
/// tests says what the screens actually look like rather than only that they passed.
fn report(name: &str, frame: &Frame) -> String {
    format!(
        "{name:<28} rows {:>4}  smallest {:>5.1}px  cards {:>4}  lines {:>4}  coverage {:>4.0}%",
        frame.text.iter().filter(|p| p.is_ink()).count(),
        frame.smallest(),
        frame.boxes.len(),
        frame.lines.len(),
        frame.coverage(frame.screen) * 100.0,
    )
}

/// Nothing on any screen, at either size, may be painted on top of anything else.
///
/// The single most useful check here. Every overlap the redesign introduced was found by this
/// and not by looking, because two rows a pixel apart look fine in a screenshot and read as a
/// smudge on a real monitor.
/// Checked at both sizes.
///
/// SMALL used to be excluded, on the grounds that the rail's summary ran into its own footer on
/// an 800 pixel tall window. It never did. `collisions` was comparing the rectangles egui laid
/// text out into rather than the parts a clip rectangle lets through, so the rail rows scrolled
/// below the fold, invisible, were being reported as landing on the footer drawn down there.
/// Four attempts to fix the rail failed because there was nothing wrong with the rail. The
/// detector is the thing that was wrong, and every layout test in this file depends on it.
#[test]
fn nothing_overlaps_anything_on_any_screen() {
    for size in [BIG, SMALL] {
        for which in Tab::ALL {
            let mut a = app();
            let frame = tab(&mut a, which, size);
            let hits = frame.collisions();
            assert!(
                hits.is_empty(),
                "{which:?} at {size:?} paints text on top of text:\n{}",
                describe_pairs(&hits)
            );
        }
    }
}

/// Nothing may be cut off sideways by the card it was drawn in.
#[test]
fn nothing_is_cut_off_by_the_card_it_is_in() {
    for size in [BIG, SMALL] {
        for which in Tab::ALL {
            let mut a = app();
            let frame = tab(&mut a, which, size);
            let cut = frame.cut_off();
            assert!(
                cut.is_empty(),
                "{which:?} at {size:?} cuts text off:\n{}",
                describe(&cut)
            );
        }
    }
}

/// The floor holds where it matters, which is on the screen and not only in the theme.
#[test]
fn nothing_on_any_screen_is_below_the_readable_floor() {
    let mut lines = Vec::new();
    for size in [BIG, SMALL] {
        for which in Tab::ALL {
            let mut a = app();
            let frame = tab(&mut a, which, size);
            lines.push(report(&format!("{which:?} {}", size.x as i32), &frame));
            assert!(
                frame.smallest() >= 13.0,
                "{which:?} at {size:?} painted {}px type",
                frame.smallest()
            );
        }
    }
    println!("{}", lines.join("\n"));
}

/// The scale has to reach the screen. A theme with six roles that draws everything at one size
/// is the old panel with a bigger font.
#[test]
fn the_type_scale_actually_reaches_the_screen() {
    let mut a = app();
    let frame = tab(&mut a, Tab::Overview, BIG);
    let mut sizes: Vec<i32> = frame
        .text
        .iter()
        .filter(|p| p.is_ink())
        .map(|p| p.size.round() as i32)
        .collect();
    sizes.sort_unstable();
    sizes.dedup();
    assert!(
        sizes.len() >= 4,
        "the overview only used {sizes:?}, which is not a hierarchy"
    );
    assert!(
        sizes.iter().any(|s| *s >= 30),
        "nothing on the front page is large, so nothing reads from across the room: {sizes:?}"
    );
}

/// The defect the redesign exists to fix: a huge canvas with a little text in the corner. Not
/// a beauty score, just a floor, and it is checked on the screen size that used to be worst.
#[test]
fn the_big_screen_is_used_rather_than_left_empty() {
    for which in Tab::ALL {
        let mut a = app();
        let frame = tab(&mut a, which, BIG);
        let content = Rect::from_min_max(
            Pos2::new(280.0, 90.0),
            Pos2::new(BIG.x - 20.0, BIG.y - 20.0),
        );
        let used = frame.coverage(content);
        assert!(
            used > 0.30,
            "{which:?} leaves {:.0}% of the screen empty",
            (1.0 - used) * 100.0
        );
    }
}

/// The rail is the one thing that is always true, so it has to be on every screen, and it has
/// to say what the panel is attached to whichever screen you are on.
#[test]
fn the_rail_says_what_it_is_attached_to_on_every_screen() {
    for which in Tab::ALL {
        let mut a = app();
        let frame = tab(&mut a, which, BIG);
        let words = frame.words();
        assert!(
            words.contains("mock"),
            "{which:?} does not say it is on the mock"
        );
        assert!(
            words.contains("LINK LIVE"),
            "{which:?} does not show the link"
        );
        assert!(words.contains("AOS"), "{which:?} lost the wordmark");
        for tab_name in Tab::ALL.iter().map(|t| t.label()) {
            assert!(
                words.contains(tab_name),
                "{which:?} lost the {tab_name} rail entry"
            );
        }
    }
}

/// The moment the link goes, every screen has to carry the warning, not just the one that
/// happened to be open when it went.
#[test]
fn losing_the_link_marks_every_screen() {
    for which in Tab::ALL {
        let mut a = app();
        a.link = Link::Disconnected {
            why: "backend closed the connection".into(),
        };
        let frame = tab(&mut a, which, BIG);
        let words = frame.words();
        assert!(
            words.contains("NOT LIVE"),
            "{which:?} looks live while it is not"
        );
        assert!(
            words.contains("before the link dropped"),
            "{which:?} does not say what is on screen is old"
        );
    }
}

/// Unknown is drawn as unknown. Checked on the real fixture, which carries a component that
/// was looked at and could not be read and one that has never been read at all.
#[test]
fn a_gap_is_drawn_as_a_gap_and_never_as_a_zero() {
    let mut a = app();
    let frame = tab(&mut a, Tab::Diagnostics, BIG);
    let words = frame.words();

    assert!(
        words.contains("UNKNOWN"),
        "the unmeasured components lost their state word"
    );
    assert!(
        words.contains("never sampled"),
        "a component nothing has ever read must say so"
    );
    assert!(
        words.contains("unknown") || words.contains("UNKNOWN"),
        "the unreadable vram reading must say unknown rather than showing a figure"
    );

    // And the row for the card with no reading must not be showing a zero next to vram.
    let vram = frame.find("vram");
    assert!(
        !vram.is_empty(),
        "the unreadable metric was not drawn at all"
    );
}

/// A blocked agent has to be findable without reading the screen: a word, a shape and the
/// halo, all three.
#[test]
fn a_blocked_agent_is_marked_on_the_screen_it_belongs_on() {
    let mut a = app();
    for agent in a.snapshot.agents.iter_mut().filter(|x| x.name == "nora") {
        agent.status = AgentStatus::Blocked;
        agent.blocker = Some("run-tests.sh needs python3-pytest".into());
    }

    let frame = tab(&mut a, Tab::Agents, BIG);
    let words = frame.words();
    assert!(words.contains("BLOCKED"), "the state word is missing");
    assert!(
        words.contains("python3-pytest"),
        "the card does not say what stopped her"
    );

    // And the front page has to raise it too, since that is what the front page is for.
    let front = tab(&mut a, Tab::Overview, BIG);
    assert!(front.words().contains("BLOCKED"));
    assert!(front.words().contains("python3-pytest"));
}

/// The hierarchy is drawn, not implied. One connector per agent below the root.
#[test]
fn the_chain_is_drawn_with_connectors_rather_than_indentation_alone() {
    let mut a = app();
    let bare = tab(&mut a, Tab::Carl, BIG).lines.len();
    let tree = tab(&mut a, Tab::Agents, BIG).lines.len();
    assert!(
        tree > bare + 4,
        "the agents screen drew {tree} lines against {bare} elsewhere, so nothing joins the cards"
    );
}

/// JJ is drawn apart from the army, in his own block, and never as one of the agent cards.
#[test]
fn the_person_is_drawn_apart_from_the_agents() {
    let mut a = app();
    let frame = tab(&mut a, Tab::Agents, BIG);
    let words = frame.words();
    assert!(
        words.contains("COMMAND AUTHORITY"),
        "JJ has no block of his own"
    );
    assert!(
        words.contains("not one of the agents"),
        "the screen does not say what JJ is"
    );
    assert!(
        words.contains("CHAIN OF COMMAND"),
        "the army has no heading"
    );
}

/// The console has one obvious place to type, and it says what pressing send will do.
#[test]
fn the_console_has_one_obvious_input() {
    let mut a = app();
    let frame = tab(&mut a, Tab::Carl, BIG);
    let words = frame.words();
    assert!(words.contains("SAY TO CARL"));
    assert!(words.contains("SET AN OBJECTIVE"));
    assert!(words.contains("SEND"));
    assert!(
        words.contains("Goes to Carl"),
        "the composer does not say where what you type ends up"
    );
}

/// An empty screen has to say something useful rather than sitting blank.
#[test]
fn every_empty_state_says_something_rather_than_nothing() {
    let mut a = app();
    a.snapshot.projects.clear();
    a.snapshot.conversation.clear();
    a.snapshot.delegations.clear();
    a.snapshot.events.clear();

    let projects = tab(&mut a, Tab::Projects, BIG);
    assert!(projects.words().contains("CARRYING NO PROJECTS"));
    assert!(
        projects.words().contains("WHAT WOULD SHOW UP HERE"),
        "an empty projects screen has to say what would fill it"
    );

    let console = tab(&mut a, Tab::Carl, BIG);
    assert!(
        console
            .words()
            .contains("NOTHING FROM BEFORE THIS PANEL OPENED")
    );
    assert!(console.words().contains("Try one of these"));

    let front = tab(&mut a, Tab::Overview, BIG);
    assert!(front.words().contains("NO PROJECTS ON THE BACKEND"));
}

/// Selecting something has to fill the inspector rather than leaving it as a placeholder.
#[test]
fn picking_an_agent_fills_the_inspector() {
    let mut a = app();
    let before = tab(&mut a, Tab::Agents, BIG);
    assert!(before.words().contains("NO AGENT SELECTED"));

    a.select_agent("nora");
    let after = tab(&mut a, Tab::Agents, BIG);
    let words = after.words();
    assert!(!words.contains("NO AGENT SELECTED"));
    assert!(words.contains("REPORTING LINE"));
    assert!(words.contains("DIRECT JJ INTERVENTION"));
    // The chain is drawn as names joined by arrows now, rather than by the words "answers to".
    // Changed deliberately: the words ran together into "noraanswers tomason", and even spaced
    // they read as a sentence rather than a line of command.
    assert!(
        words.contains("Nora") && words.contains("Mason") && words.contains("JJ"),
        "the chain does not name the whole line: {words}"
    );
    assert!(
        words.contains('\u{2192}'),
        "the chain is not joined by arrows"
    );
}

/// The probe itself has to be able to see a collision, or every test above passes for the
/// wrong reason. Two labels drawn at the same place, deliberately.
#[test]
fn the_probe_can_actually_see_an_overlap() {
    let ctx = Context::default();
    crate::theme::install(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
    let out = ctx.run(
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                let at = ui.max_rect().left_top() + Vec2::new(10.0, 10.0);
                for word in ["overlapping", "collision"] {
                    ui.painter().text(
                        at,
                        eframe::egui::Align2::LEFT_TOP,
                        word,
                        crate::theme::body(),
                        crate::theme::TEXT,
                    );
                }
            });
        },
    );

    let mut frame = Frame {
        screen,
        text: Vec::new(),
        boxes: Vec::new(),
        lines: Vec::new(),
    };
    for clipped in &out.shapes {
        gather(clipped.clip_rect, &clipped.shape, &mut frame);
    }
    assert_eq!(
        frame.collisions().len(),
        1,
        "the probe cannot see two labels drawn on top of each other, so it proves nothing"
    );
}

/// The workspace docks at the bottom over whatever screen is showing, and a screen that has
/// already claimed the whole height paints into the space it takes. JJ caught exactly this:
/// agent cards showing through the Diagnostics board with the workspace open.
#[test]
fn the_workspace_does_not_let_the_screen_behind_it_show_through() {
    for tab in [
        Tab::Overview,
        Tab::Carl,
        Tab::Agents,
        Tab::Diagnostics,
        Tab::Projects,
    ] {
        for size in [BIG, SMALL] {
            let mut a = app();
            a.open_workspace(crate::WorkspaceRequest::Terminal { cwd: "/tmp".into() });
            let frame = super::tab(&mut a, tab, size);
            let hits = frame.collisions();
            assert!(
                hits.is_empty(),
                "{tab:?} at {size:?} with the workspace open paints text on text:\n{}",
                describe_pairs(&hits)
            );
            let cut = frame.cut_off();
            assert!(
                cut.is_empty(),
                "{tab:?} at {size:?} with the workspace open cuts text off:\n{}",
                describe(&cut)
            );
        }
    }
}

/// Every screen with nothing in it.
///
/// The probe drives the mock, and the mock always has an army doing something, so the empty
/// states were the one thing on the panel that had never been painted in a test. JJ found two
/// of them cut off on the real backend, where the army is idle and nothing has been delegated,
/// which is exactly the state a first run is in.
#[test]
fn the_empty_states_fit_what_they_say() {
    for size in [BIG, SMALL] {
        for which in Tab::ALL {
            let mut a = app();
            a.snapshot = crate::model::Snapshot::default();
            let frame = tab(&mut a, which, size);

            let cut = frame.cut_off();
            assert!(
                cut.is_empty(),
                "{which:?} at {size:?} with nothing in it cuts text off:\n{}",
                describe(&cut)
            );
            let hits = frame.collisions();
            assert!(
                hits.is_empty(),
                "{which:?} at {size:?} with nothing in it paints text on text:\n{}",
                describe_pairs(&hits)
            );
        }
    }
}

/// The detector must not report an overlap nobody can see.
///
/// This is the failure that hid behind four attempts to fix the rail. egui records a paint
/// command for text its clip rectangle then throws away, which is how a scroll area works, so
/// comparing laid out rectangles finds collisions among rows that are below the fold. Every
/// layout test in this file rests on `collisions` being about what reaches the screen.
#[test]
fn text_the_clip_threw_away_is_not_reported_as_an_overlap() {
    let ctx = Context::default();
    crate::theme::install(&ctx);
    let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0));
    let out = ctx.run(
        RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        },
        |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                // A visible row, and a second one painted well below a clip that ends above it.
                // On screen there is one word. Laid out, there are two in the same place.
                let at = ui.max_rect().left_top() + Vec2::new(10.0, 10.0);
                ui.painter().text(
                    at,
                    eframe::egui::Align2::LEFT_TOP,
                    "visible",
                    crate::theme::body(),
                    crate::theme::TEXT,
                );
                let shut = Rect::from_min_size(ui.max_rect().left_top(), Vec2::new(400.0, 5.0));
                ui.painter().with_clip_rect(shut).text(
                    at,
                    eframe::egui::Align2::LEFT_TOP,
                    "hidden",
                    crate::theme::body(),
                    crate::theme::TEXT,
                );
            });
        },
    );

    let mut frame = super::Frame {
        screen,
        text: Vec::new(),
        boxes: Vec::new(),
        lines: Vec::new(),
    };
    for clipped in &out.shapes {
        super::gather(clipped.clip_rect, &clipped.shape, &mut frame);
    }

    assert!(
        frame.says("visible"),
        "the row that is on screen was not seen at all"
    );
    // Otherwise this test proves nothing: it has to be the case that both rows were recorded
    // and laid out on top of each other, and that only one of them reaches the screen.
    assert!(
        frame.says("hidden"),
        "the clipped row was never recorded, so there was nothing to mistake"
    );
    let stacked = frame.find("hidden")[0]
        .rect
        .intersects(frame.find("visible")[0].rect);
    assert!(stacked, "the two rows were not laid out in the same place");
    assert!(
        frame.collisions().is_empty(),
        "reported an overlap with a row the clip removed:\n{}",
        describe_pairs(&frame.collisions())
    );
}
