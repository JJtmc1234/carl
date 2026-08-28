//! Tests build a snapshot by starting from the default and setting the one field
//! under test, which reads far better here than restating every field of a large
//! struct in each case.
#![allow(clippy::field_reassign_with_default)]

//! What the recent activity feed has to get right.

use super::*;
use carl::army::event::Record;
use carl::army::task::TaskId;

use crate::model::{Delegation, Turn};
use crate::source::{MockPanelDataSource, PanelDataSource};

fn record(at: u64, actor: &str, event: Event) -> Record {
    Record {
        seq: at,
        at,
        actor: actor.into(),
        event,
    }
}

/// Four sources, one list, newest first. A feed that keeps journal records apart from
/// milestones makes somebody read two lists and work out the order themselves.
#[test]
fn every_source_lands_in_one_list_newest_first() {
    let mut s = Snapshot::default();
    s.events = vec![record(
        100,
        "nora",
        Event::Submitted {
            task: TaskId::quoted("t1"),
            attempt: 1,
            words: 12,
        },
    )];
    s.delegations = vec![Delegation {
        at: 300,
        from: "mason".into(),
        to: "nora".into(),
        goal: "check the ratios".into(),
        task: None,
    }];
    s.conversation = vec![Turn {
        at: 200,
        from: Speaker::Carl,
        text: "Handed to Adrian.".into(),
        streaming: false,
    }];

    let beats = recent(&s, 10);
    let times: Vec<u64> = beats.iter().map(|b| b.at).collect();
    assert_eq!(times, vec![300, 200, 100], "newest first");
    let kinds: Vec<&str> = beats.iter().map(|b| b.kind).collect();
    assert_eq!(kinds, vec!["HANDED DOWN", "CARL SAID", "SUBMITTED"]);
}

/// A quiet army produces an empty feed rather than filler. Anything invented here would be
/// the exact failure the whole panel is built to avoid.
#[test]
fn a_quiet_army_produces_no_beats() {
    assert!(recent(&Snapshot::default(), 10).is_empty());
}

/// The cap is a cap on what is drawn, and it must take the newest rather than the first ones
/// it happened to see.
#[test]
fn the_cap_keeps_the_newest() {
    let mut s = Snapshot::default();
    s.events = (1..=20)
        .map(|n| {
            record(
                n * 10,
                "nora",
                Event::Decided {
                    task: None,
                    what: format!("thing {n}"),
                },
            )
        })
        .collect();

    let beats = recent(&s, 5);
    assert_eq!(beats.len(), 5);
    assert_eq!(beats[0].at, 200);
    assert_eq!(beats[4].at, 160);
}

/// JJ going around the chain is the one act the whole army is arranged to avoid, so it must
/// never read as an ordinary line in the feed.
#[test]
fn an_intervention_is_marked_as_one() {
    let mut s = Snapshot::default();
    s.events = vec![record(
        50,
        "jj",
        Event::Intervened {
            what: Intervention::Message {
                to: "nora".into(),
                what: "skip the wider suite".into(),
            },
        },
    )];

    let beat = &recent(&s, 5)[0];
    assert_eq!(beat.kind, "JJ ACTED");
    assert_eq!(beat.color, theme::INTERVENE);
    assert!(beat.what.contains("nora"), "{}", beat.what);
}

/// Every kind of record has to produce a sentence. A variant nobody wrote a line for shows up
/// as an empty row, which reads as a bug in the panel rather than as a gap in the journal.
#[test]
fn every_record_produces_a_sentence() {
    let task = TaskId::quoted("t1");
    let all = [
        Event::Delegated {
            task: task.clone(),
            to: "nora".into(),
            goal: "fix belts".into(),
            parent: None,
            must: vec![],
            project: None,
            workspace: None,
            objective: None,
        },
        Event::Moved {
            task: task.clone(),
            from: "in hand".into(),
            to: "submitted".into(),
        },
        Event::Submitted {
            task: task.clone(),
            attempt: 1,
            words: 10,
        },
        Event::Reviewed {
            task: task.clone(),
            accepted: true,
            why: "tests pass".into(),
        },
        Event::Refused {
            what: "skip the chain".into(),
            why: "carl cannot reach nora".into(),
        },
        Event::EmergencyDeclared {
            task: task.clone(),
            why: "nobody else is up".into(),
        },
        Event::Decided {
            task: None,
            what: "took the narrower proof".into(),
        },
        Event::Intervened {
            what: Intervention::Objective {
                what: "smelting ratios".into(),
            },
        },
        Event::Notified {
            who: "mason".into(),
            about: 4,
        },
    ];

    for event in all {
        let line = describe(&event);
        assert!(!line.trim().is_empty(), "{:?} has no words", event.kind());
        assert!(!kind_of(&event).is_empty());
    }
}

/// A long turn is cut down rather than allowed to push everything else off the row, and the
/// cut is marked so nobody reads a truncated sentence as the whole of what was said.
#[test]
fn a_long_turn_is_cut_and_says_it_was_cut() {
    let mut s = Snapshot::default();
    s.conversation = vec![Turn {
        at: 1,
        from: Speaker::Jj,
        text: "word ".repeat(80),
        streaming: false,
    }];
    let line = &recent(&s, 1)[0].what;
    assert!(line.ends_with("..."), "{line}");
    assert!(line.chars().count() < 110, "{}", line.chars().count());
}

/// The real fixture, so the overview has something in it the moment the panel opens rather
/// than an empty pane with a heading over it.
#[test]
fn the_mock_army_has_a_feed_on_the_first_frame() {
    let s = MockPanelDataSource::new().snapshot();
    let beats = recent(&s, 12);
    assert!(
        beats.len() >= 4,
        "the opening snapshot should already show a conversation, a handover and milestones"
    );
    assert!(beats.iter().any(|b| b.kind == "MILESTONE"));
    assert!(beats.iter().any(|b| b.kind == "HANDED DOWN"));
}
