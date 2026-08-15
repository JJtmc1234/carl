//! The live adapter, checked without a backend.
//!
//! The parts worth testing are the ones that decide what the screen believes: how a health
//! reading becomes a link state, what a command turns into on the wire, and what a journal
//! frame does to what is held. None of those need a socket, and testing them through one would
//! mean the failures arrive as timeouts instead of as assertions.

use carl::army::event::{Event as JournalEvent, Record};
use carl::panel::live::Health;
use carl::panel::view::{Maybe, TaskView};
use carl::panel::wire::{Entity, PanelEvent as WireEvent};

use super::*;
use crate::command::{Intervention, InterventionKind, WorkspaceRequest};
use crate::model::{AgentView, Snapshot};

fn task(id: &str, status: &str) -> TaskView {
    TaskView {
        id: id.into(),
        goal: "fix the belt rate".into(),
        owner: "nora".into(),
        assigner: "mason".into(),
        parent: None,
        status: status.into(),
        attempts: 0,
        must: vec!["tests pass".into()],
        review: Maybe::Unknown,
        delegated_at: 10,
        updated_at: 20,
    }
}

fn frame(kind: &str, actor: &str, event: JournalEvent) -> WireEvent {
    WireEvent {
        seq: 5,
        at: 900,
        kind: kind.into(),
        entity: Entity::Agent { name: actor.into() },
        record: Record {
            seq: 5,
            at: 900,
            actor: actor.into(),
            event,
        },
    }
}

/// The four connection states the screen already draws, taken from the client rather than
/// guessed at. Only one of them may claim to be live.
#[test]
fn every_health_maps_to_a_link_and_only_connected_is_live() {
    assert!(link_of(Health::Connected).is_live());
    assert!(!link_of(Health::Reconnecting).is_live());
    assert!(
        !link_of(Health::Stale).is_live(),
        "stale means what is on screen may already be wrong"
    );
    assert!(!link_of(Health::Disconnected).is_live());

    // And each says something different, or the badge tells JJ nothing.
    let labels: Vec<String> = [
        Health::Connected,
        Health::Reconnecting,
        Health::Stale,
        Health::Disconnected,
    ]
    .iter()
    .map(|h| link_of(*h).label())
    .collect();
    assert_eq!(labels[0], "LINK LIVE");
    assert_ne!(labels[1], labels[3]);
}

/// What JJ meant becomes what the backend does, with no widget ever building a wire type.
#[test]
fn every_command_has_a_backend_meaning() {
    use carl::panel::PanelCommand;

    assert!(matches!(
        translate::to_wire(&Command::SayToCarl("hello".into())),
        Some(PanelCommand::Say { .. })
    ));
    assert!(matches!(
        translate::to_wire(&Command::SetObjective("fix it".into())),
        Some(PanelCommand::Objective { .. })
    ));

    let answered = translate::to_wire(&Command::AnswerDecision {
        id: "7".into(),
        answer: "yes".into(),
    });
    match answered {
        Some(PanelCommand::Answer { seq, text }) => {
            assert_eq!(seq, 7, "the decision id is the sequence that asked");
            assert_eq!(text, "yes");
        }
        other => panic!("expected an answer, got {other:?}"),
    }

    // The workspace is the panel's own container and never reaches the army's channel.
    assert!(
        translate::to_wire(&Command::Workspace(WorkspaceRequest::Close)).is_none(),
        "opening a pane is not something to ask the army for"
    );
}

/// Each of the four interventions has to reach a different backend command, or three of the
/// buttons are decoration.
#[test]
fn the_four_interventions_are_four_different_commands() {
    use carl::panel::PanelCommand;

    let of = |kind| {
        translate::to_wire(&Command::Intervene(Intervention {
            agent: "nora".into(),
            kind,
            body: "stop that".into(),
        }))
    };

    assert!(matches!(
        of(InterventionKind::Message),
        Some(PanelCommand::JjMessage { .. })
    ));
    assert!(matches!(
        of(InterventionKind::ChangeInstruction),
        Some(PanelCommand::JjInstruct { .. })
    ));
    assert!(matches!(
        of(InterventionKind::StopTask),
        Some(PanelCommand::JjStop { .. })
    ));
    assert!(matches!(
        of(InterventionKind::ReplaceTask),
        Some(PanelCommand::JjReplace { .. })
    ));

    // And every one of them is an intervention as far as the backend is concerned, which is
    // what gets it recorded as JJ going around the chain.
    for kind in InterventionKind::ALL {
        assert!(
            of(kind).expect("a command").is_intervention(),
            "{kind:?} must be recorded as an intervention"
        );
    }
}

/// A journal frame moves what it actually states and nothing else.
#[test]
fn a_moved_record_moves_that_task_and_leaves_the_rest_alone() {
    let mut held = Snapshot {
        tasks: vec![task("t1", "in hand"), task("t2", "assigned")],
        agents: vec![AgentView::unknown("nora")],
        ..Default::default()
    };
    let mut out = Vec::new();

    translate::from_event(
        &frame(
            "moved",
            "nora",
            JournalEvent::Moved {
                task: carl::army::task::TaskId::quoted("t1"),
                from: "in hand".into(),
                to: "submitted".into(),
            },
        ),
        &mut held,
        &mut out,
    );

    assert_eq!(held.task("t1").unwrap().status, "submitted");
    assert_eq!(
        held.task("t2").unwrap().status,
        "assigned",
        "a frame about one task must not touch another"
    );
    assert!(out.iter().any(|e| matches!(e, PanelEvent::Recorded(_))));
    assert!(out.iter().any(|e| matches!(e, PanelEvent::TaskChanged(_))));
}

/// The record is always kept, even when nothing else can be derived from it, because the agent
/// detail view lists records and that is true regardless.
#[test]
fn a_record_nothing_can_be_derived_from_is_still_recorded() {
    let mut held = Snapshot::default();
    let mut out = Vec::new();

    translate::from_event(
        &frame(
            "refused",
            "carl",
            JournalEvent::Refused {
                what: "delegate to nora".into(),
                why: "not a direct report".into(),
            },
        ),
        &mut held,
        &mut out,
    );

    assert_eq!(out.len(), 1);
    assert!(matches!(out[0], PanelEvent::Recorded(_)));
}

/// The one live overlay a record does support is who just did something and when.
#[test]
fn a_frame_marks_its_agent_as_having_just_acted() {
    let mut held = Snapshot {
        agents: vec![AgentView::unknown("nora")],
        ..Default::default()
    };
    let mut out = Vec::new();

    translate::from_event(
        &frame(
            "submitted",
            "nora",
            JournalEvent::Submitted {
                task: carl::army::task::TaskId::quoted("t1"),
                attempt: 2,
                words: 100,
            },
        ),
        &mut held,
        &mut out,
    );

    let nora = held.agent("nora").unwrap();
    assert_eq!(nora.last_activity.as_deref(), Some("submitted"));
    assert_eq!(nora.last_activity_at, Some(900));
    assert!(out.iter().any(|e| matches!(e, PanelEvent::AgentChanged(_))));
}
