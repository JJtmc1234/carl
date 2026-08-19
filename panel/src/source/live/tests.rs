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
        project: carl::ProjectId::new("jjtorio").ok(),
        status: status.into(),
        attempts: 0,
        must: vec!["tests pass".into()],
        review: Maybe::Unknown,
        delegated_at: 10,
        updated_at: 20,
    }
}

fn frame(kind: &str, actor: &str, event: JournalEvent) -> WireEvent {
    frame_at(5, kind, actor, event)
}

fn frame_at(seq: u64, kind: &str, actor: &str, event: JournalEvent) -> WireEvent {
    WireEvent {
        seq,
        at: 900,
        kind: kind.into(),
        entity: Entity::Agent { name: actor.into() },
        record: Record {
            seq,
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

/// The frame loop must never wait on the backend.
///
/// `poll` drains whatever has arrived and returns, and with nothing there it returns nothing.
/// If it ever blocked, the panel would freeze whenever the army went quiet, which is most of
/// the time.
#[test]
fn draining_returns_at_once_when_the_backend_is_silent() {
    let (mut source, _tx, _orders) = LivePanelDataSource::detached(Snapshot::default());

    let began = std::time::Instant::now();
    let got = source.poll();
    let took = began.elapsed();

    assert!(got.is_empty(), "nothing arrived, so nothing comes back");
    assert!(
        took < std::time::Duration::from_millis(50),
        "poll blocked for {took:?}, which would freeze the frame loop"
    );
}

/// A health reading changes the badge and nothing else.
#[test]
fn a_health_update_changes_only_the_link() {
    let before = Snapshot {
        agents: vec![AgentView::unknown("nora")],
        ..Default::default()
    };
    let (mut source, tx, _orders) = LivePanelDataSource::detached(before);

    tx.send(FromBackend::Update(Box::new(Update::Health(
        Health::Reconnecting,
    ))))
    .unwrap();
    let got = source.poll();

    assert!(!source.link().is_live());
    assert!(matches!(got.as_slice(), [PanelEvent::LinkChanged(_)]));
    assert_eq!(
        source.snapshot().agents.len(),
        1,
        "the last state is kept for reference rather than blanked"
    );
}

/// Nothing may be sent while the link is down, and it must not be queued for later either.
#[test]
fn a_command_is_refused_out_loud_while_disconnected_and_never_queued() {
    let (mut source, tx, orders) = LivePanelDataSource::detached(Snapshot::default());

    tx.send(FromBackend::Update(Box::new(Update::Health(
        Health::Disconnected,
    ))))
    .unwrap();
    source.poll();

    let refused = source
        .submit(Command::SayToCarl("are you there".into()))
        .unwrap_err();
    assert!(refused.contains("not sent"), "{refused}");
    assert!(
        orders.try_recv().is_err(),
        "a refused command must not be sitting in the queue waiting to surprise somebody"
    );
}

/// Carl's answer arrives in pieces and the turn closes only when the backend stops talking.
#[test]
fn carl_streams_in_chunks_and_the_caret_goes_out_at_the_end() {
    let (mut source, tx, _orders) = LivePanelDataSource::detached(Snapshot::default());

    tx.send(FromBackend::Speaking("Handed to ".into())).unwrap();
    tx.send(FromBackend::Speaking("Adrian.".into())).unwrap();
    let mid = source.poll();
    assert_eq!(mid.len(), 2);
    assert!(
        mid.iter()
            .all(|e| matches!(e, PanelEvent::CarlSaid { streaming, .. } if *streaming))
    );

    tx.send(FromBackend::Settled(Ok(()))).unwrap();
    let end = source.poll();
    assert!(
        matches!(
            end.as_slice(),
            [PanelEvent::CarlSaid {
                streaming: false,
                ..
            }]
        ),
        "the end of the answer must close the turn: {end:?}"
    );
}

/// A command that failed is reported rather than swallowed, and does not close a turn nobody
/// started.
#[test]
fn a_failed_command_is_surfaced() {
    let (mut source, tx, _orders) = LivePanelDataSource::detached(Snapshot::default());

    tx.send(FromBackend::Settled(Err("backend refused".into())))
        .unwrap();
    let got = source.poll();

    assert!(matches!(got.as_slice(), [PanelEvent::CommandRefused(_)]));
}

/// Telemetry is the machine being sampled, not the army doing something.
///
/// It must replace the readings and leave every other part of the model exactly where it was.
/// The failure this guards against is subtle and expensive: telemetry arriving on the event
/// timeline shows a row saying an agent acted when nobody did, and telemetry moving the
/// sequence makes the panel ask the backend to resume from a point the journal never reached.
#[test]
fn telemetry_replaces_readings_and_touches_nothing_else() {
    use carl::providers::health::{Diagnostic, Health, Kind};

    let mut held = Snapshot {
        agents: vec![AgentView::unknown("nora")],
        tasks: vec![task("t1", "in hand")],
        diagnostics: vec![
            Diagnostic::new("system.cpu", Health::Healthy, "load 2.1", Kind::Sampled).measured(100),
            Diagnostic::new(
                "army.tasks",
                Health::Healthy,
                "1 in hand",
                Kind::EventDriven,
            ),
        ],
        events: Vec::new(),
        ..Default::default()
    };

    let fresh = vec![
        Diagnostic::new("system.cpu", Health::Degraded, "load 7.8", Kind::Sampled).measured(500),
    ];
    translate::replace_telemetry(&mut held, &fresh);

    let cpu = held
        .diagnostics
        .iter()
        .find(|d| d.component == "system.cpu")
        .expect("the cpu row");
    assert_eq!(cpu.health, Health::Degraded, "the reading was replaced");
    assert_eq!(cpu.measured_at, Some(500));

    // The army rows are untouched, and a sampler that only reads the machine must not delete
    // the state beside it.
    assert_eq!(held.diagnostics.len(), 2, "the army row survived");
    assert!(held.events.is_empty(), "telemetry is not a journal record");
    assert_eq!(held.tasks[0].status, "in hand", "no task reducer ran");
    assert_eq!(held.agents[0].last_activity, None, "nobody acted");
}

/// A component the sample did not mention is left alone rather than dropped.
#[test]
fn a_reading_that_was_not_sampled_is_kept() {
    use carl::providers::health::{Diagnostic, Health, Kind};

    let mut held = Snapshot {
        diagnostics: vec![Diagnostic::new(
            "system.gpu",
            Health::Unknown,
            "no card",
            Kind::Sampled,
        )],
        ..Default::default()
    };

    translate::replace_telemetry(
        &mut held,
        &[Diagnostic::new(
            "system.cpu",
            Health::Healthy,
            "fine",
            Kind::Sampled,
        )],
    );

    assert_eq!(held.diagnostics.len(), 2);
    assert!(
        held.diagnostics.iter().any(|d| d.component == "system.gpu"),
        "an unmentioned component must not vanish"
    );
}

/// The two reducers are separate functions and stay that way.
///
/// Driven through the source so the ordering is the real one: telemetry in, then an event, and
/// neither has done the other's job.
#[test]
fn the_event_reducer_and_the_telemetry_reducer_do_not_meet() {
    use carl::providers::health::{Diagnostic, Health, Kind};

    let held = Snapshot {
        agents: vec![AgentView::unknown("nora")],
        tasks: vec![task("t1", "in hand")],
        ..Default::default()
    };
    let (mut source, tx, _orders) = LivePanelDataSource::detached(held);

    tx.send(FromBackend::Update(Box::new(Update::Telemetry {
        at: 500,
        diagnostics: vec![
            Diagnostic::new("system.cpu", Health::Degraded, "load high", Kind::Sampled)
                .measured(500),
        ],
    })))
    .unwrap();

    let from_telemetry = source.poll();
    assert!(
        matches!(
            from_telemetry.as_slice(),
            [PanelEvent::TelemetryChanged { at: 500, .. }]
        ),
        "telemetry produces exactly one telemetry update: {from_telemetry:?}"
    );
    assert!(
        !from_telemetry
            .iter()
            .any(|e| matches!(e, PanelEvent::Recorded(_) | PanelEvent::TaskChanged(_))),
        "and never anything that belongs to the army timeline"
    );

    tx.send(FromBackend::Update(Box::new(Update::Event(Box::new(
        frame(
            "moved",
            "nora",
            JournalEvent::Moved {
                task: carl::army::task::TaskId::quoted("t1"),
                from: "in hand".into(),
                to: "submitted".into(),
            },
        ),
    )))))
    .unwrap();

    let from_event = source.poll();
    assert!(
        from_event
            .iter()
            .any(|e| matches!(e, PanelEvent::Recorded(_))),
        "an event still produces a record"
    );
    assert!(
        !from_event
            .iter()
            .any(|e| matches!(e, PanelEvent::TelemetryChanged { .. })),
        "and never telemetry"
    );
}

/// The sequence is what a reconnection resumes from, and telemetry has none.
///
/// Walked in the exact order the two kinds arrive in real life, because the failure this
/// prevents is not that a number is wrong on screen. It is that a number pushed past the
/// journal makes the panel ask the backend to continue from a record that never existed, and
/// everything between is skipped without anybody noticing.
#[test]
fn telemetry_never_moves_the_sequence_and_events_always_do() {
    use carl::providers::health::{Diagnostic, Health, Kind};

    let (mut source, tx, _orders) = LivePanelDataSource::detached(Snapshot::default());

    let sample = || {
        FromBackend::Update(Box::new(Update::Telemetry {
            at: 1_000,
            diagnostics: vec![
                Diagnostic::new("system.cpu", Health::Healthy, "fine", Kind::Sampled)
                    .measured(1_000),
            ],
        }))
    };

    // Caught up to seven.
    tx.send(FromBackend::Update(Box::new(Update::Event(Box::new(
        frame_at(
            7,
            "moved",
            "nora",
            JournalEvent::Refused {
                what: "x".into(),
                why: "y".into(),
            },
        ),
    )))))
    .unwrap();
    source.poll();
    assert_eq!(source.last_seq(), 7);

    // Telemetry arrives. It must not move.
    tx.send(sample()).unwrap();
    source.poll();
    assert_eq!(source.last_seq(), 7, "telemetry moved the sequence");

    // Event eight arrives. It must move.
    tx.send(FromBackend::Update(Box::new(Update::Event(Box::new(
        frame_at(
            8,
            "moved",
            "nora",
            JournalEvent::Refused {
                what: "x".into(),
                why: "y".into(),
            },
        ),
    )))))
    .unwrap();
    source.poll();
    assert_eq!(source.last_seq(), 8);

    // Telemetry again. Still must not move.
    tx.send(sample()).unwrap();
    source.poll();
    assert_eq!(source.last_seq(), 8, "telemetry moved the sequence");

    // And however much of it arrives, in a burst, it is still eight.
    for _ in 0..20 {
        tx.send(sample()).unwrap();
    }
    source.poll();
    assert_eq!(
        source.last_seq(),
        8,
        "a burst of telemetry moved the sequence"
    );
}

/// A resync carries its own sequence and the stream continues from exactly there, so it is the
/// one thing besides an event that may move it.
#[test]
fn a_resync_sets_the_sequence_to_the_snapshot_it_carries() {
    let (mut source, tx, _orders) = LivePanelDataSource::detached(Snapshot::default());

    tx.send(FromBackend::Update(Box::new(Update::Event(Box::new(
        frame_at(
            3,
            "moved",
            "nora",
            JournalEvent::Refused {
                what: "x".into(),
                why: "y".into(),
            },
        ),
    )))))
    .unwrap();
    source.poll();
    assert_eq!(source.last_seq(), 3);

    tx.send(FromBackend::Update(Box::new(Update::Resynced(Box::new(
        wire_snapshot(41),
    )))))
    .unwrap();
    source.poll();
    assert_eq!(
        source.last_seq(),
        41,
        "a resync joins the stream at its own sequence"
    );
}

/// A backend snapshot with a given sequence and nothing else in it.
fn wire_snapshot(seq: u64) -> carl::panel::view::PanelSnapshot {
    carl::panel::view::PanelSnapshot {
        seq,
        at: 1,
        carl: carl::panel::view::CarlView {
            status: Maybe::Unknown,
            pending: Vec::new(),
            objectives: Vec::new(),
            recent_delegations: Vec::new(),
        },
        agents: Vec::new(),
        tasks: Vec::new(),
        projects: Vec::new(),
        diagnostics: Vec::new(),
    }
}
