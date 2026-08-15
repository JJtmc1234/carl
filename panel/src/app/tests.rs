//! What the panel has to get right, checked without opening a window.
//!
//! All of this is possible only because `App` has no egui in it. A UI whose rules live inside
//! its draw calls can only be tested by rendering it and looking, which means in practice it
//! is not tested at all.

use std::time::Duration;

use super::*;
use crate::command::{Command, InterventionKind, WorkspaceRequest};
use crate::model::{AgentStatus, AgentView, Decision, Diagnostic, Health, Link, Milestone};
use crate::source::{MockPanelDataSource, PanelEvent};

fn app() -> App {
    App::new(Box::new(MockPanelDataSource::new()))
}

#[test]
fn the_panel_opens_on_carl_with_a_real_snapshot() {
    let a = app();
    assert_eq!(a.tab, Tab::Carl, "the conversation is the front page");
    assert!(!a.snapshot.agents.is_empty());
    assert!(!a.snapshot.conversation.is_empty());
    assert!(a.link.is_live());

    // The agents are the real organisation, not a list invented for the panel.
    for agent in carl::army::org::everyone() {
        assert!(
            a.snapshot.agent(agent.name).is_some(),
            "{} is missing from the snapshot",
            agent.name
        );
    }
}

#[test]
fn every_tab_can_be_selected() {
    let mut a = app();
    for tab in Tab::ALL {
        a.select_tab(tab);
        assert_eq!(a.tab, tab);
    }
    assert_eq!(Tab::ALL.len(), 4, "exactly four principal tabs");
}

/// The editor and the terminal are tools, not destinations. If either ever becomes a tab this
/// fails, which is the point.
#[test]
fn the_editor_and_terminal_are_not_tabs() {
    let labels: Vec<&str> = Tab::ALL.iter().map(|t| t.label()).collect();
    assert_eq!(labels, vec!["CARL", "AGENTS", "DIAGNOSTICS", "PROJECTS"]);
}

/// Toggling away and back is not a restart. A panel that forgets where you were is one you
/// stop toggling, which defeats having a shortcut at all.
#[test]
fn hiding_and_showing_keeps_everything_worth_keeping() {
    let mut a = app();
    a.select_tab(Tab::Agents);
    a.select_agent("nora");
    a.select_project("jjtorio");
    a.open_workspace(WorkspaceRequest::Terminal {
        cwd: "/home/jj/x".into(),
    });
    a.draft = "half typed message".into();
    a.objective = "half typed objective".into();
    a.conversation_at_end = false;

    let before = a.kept();

    a.toggle_visible();
    assert!(!a.visible, "hidden");
    a.toggle_visible();
    assert!(a.visible, "and back");

    assert_eq!(a.kept(), before, "nothing was lost across the toggle");
    assert_eq!(a.tab, Tab::Agents);
    assert_eq!(a.agent.as_deref(), Some("nora"));
    assert_eq!(a.project.as_deref(), Some("jjtorio"));
    assert!(a.workspace.is_some());
    assert_eq!(a.draft, "half typed message");
}

/// Events must reach the screen without anybody asking for them.
#[test]
fn a_live_agent_change_lands_without_a_refresh() {
    let mut a = app();
    assert_eq!(a.snapshot.agent("nora").unwrap().status, AgentStatus::Idle);

    let mut nora = AgentView::unknown("nora");
    nora.status = AgentStatus::Working;
    nora.last_activity = Some("reading belts.py".into());
    a.apply(PanelEvent::AgentChanged(Box::new(nora)));

    let after = a.snapshot.agent("nora").unwrap();
    assert_eq!(after.status, AgentStatus::Working);
    assert_eq!(after.last_activity.as_deref(), Some("reading belts.py"));
    assert!(a.is_lit("nora"), "a change that just landed is marked");
}

/// A blocker has to arrive as a blocker, since that is the state the whole screen is arranged
/// to surface.
#[test]
fn a_blocker_arrives_live_and_is_visible_as_one() {
    let mut a = app();
    let mut nora = AgentView::unknown("nora");
    nora.status = AgentStatus::Blocked;
    nora.blocker = Some("a dependency is missing".into());
    a.apply(PanelEvent::AgentChanged(Box::new(nora)));

    let after = a.snapshot.agent("nora").unwrap();
    assert!(after.status.wants_attention());
    assert_eq!(after.blocker.as_deref(), Some("a dependency is missing"));
}

/// Carl's answer arrives in pieces and must stay one turn, with the caret going out only when
/// the last piece says it is finished.
#[test]
fn a_streamed_answer_is_one_turn_that_finishes() {
    let mut a = app();
    let before = a.snapshot.conversation.len();

    a.apply(PanelEvent::CarlSaid {
        text: "Handed to Adrian".into(),
        streaming: true,
    });
    assert_eq!(a.snapshot.conversation.len(), before + 1);
    assert!(a.streaming_turn().is_some(), "still writing");

    a.apply(PanelEvent::CarlSaid {
        text: ", who is routing it to Mason.".into(),
        streaming: false,
    });
    assert_eq!(
        a.snapshot.conversation.len(),
        before + 1,
        "one answer, not two"
    );
    let last = a.snapshot.conversation.last().unwrap();
    assert_eq!(last.text, "Handed to Adrian, who is routing it to Mason.");
    assert!(!last.streaming);
    assert!(a.streaming_turn().is_none(), "the caret goes out");
}

/// A decision has to appear live and disappear when it is answered.
#[test]
fn a_pending_decision_appears_and_clears() {
    let mut a = app();
    assert!(a.snapshot.decisions.is_empty());

    a.apply(PanelEvent::DecisionRaised(Box::new(Decision {
        id: "d1".into(),
        asked_at: 10,
        question: "Accept the narrower proof?".into(),
        detail: None,
        options: vec!["Yes".into(), "Hold".into()],
    })));
    assert_eq!(a.snapshot.decisions.len(), 1);

    // The same decision arriving twice is still one decision.
    a.apply(PanelEvent::DecisionRaised(Box::new(Decision {
        id: "d1".into(),
        asked_at: 11,
        question: "Accept the narrower proof?".into(),
        detail: None,
        options: vec![],
    })));
    assert_eq!(a.snapshot.decisions.len(), 1, "not duplicated");

    a.apply(PanelEvent::DecisionSettled { id: "d1".into() });
    assert!(a.snapshot.decisions.is_empty());
}

/// The link going down must be visible, and coming back must replace the world rather than
/// continue the old one, because nothing filled the gap.
#[test]
fn a_reconnect_replaces_the_world_rather_than_patching_it() {
    let mut a = app();

    // Something that only exists on screen, from before the drop.
    let mut ghost = AgentView::unknown("nora");
    ghost.status = AgentStatus::Blocked;
    ghost.blocker = Some("from before the link went".into());
    a.apply(PanelEvent::AgentChanged(Box::new(ghost)));
    assert!(a.snapshot.agent("nora").unwrap().blocker.is_some());

    a.apply(PanelEvent::LinkChanged(Link::Disconnected {
        why: "backend closed".into(),
    }));
    assert!(!a.link.is_live());
    assert!(
        a.snapshot.agent("nora").unwrap().blocker.is_some(),
        "the stale view is kept and shown as stale rather than blanked"
    );

    a.apply(PanelEvent::LinkChanged(Link::Connecting { attempt: 1 }));
    assert!(!a.link.is_live());

    a.apply(PanelEvent::LinkChanged(Link::Live));
    assert!(a.link.is_live());
    assert!(
        a.snapshot.agent("nora").unwrap().blocker.is_none(),
        "coming back takes a fresh snapshot rather than keeping what was on screen"
    );
    assert!(a.resynced_at.is_some(), "and says that it did");
    assert!(a.lit.is_empty(), "old highlights do not survive a resync");
}

/// A message is a command going out, and nothing appears on screen until the backend echoes it.
#[test]
fn sending_a_message_generates_a_command_and_draws_nothing_locally() {
    let mut source = MockPanelDataSource::new();
    let before = source.snapshot().conversation.len();
    let mut a = App::new(Box::new(MockPanelDataSource::new()));
    let _ = &before;

    a.draft = "  fix the belt numbers  ".into();
    a.send_draft();

    assert!(a.draft.is_empty(), "the box is cleared");
    assert_eq!(
        a.snapshot.conversation.len(),
        before,
        "nothing is drawn until the backend echoes it"
    );

    // The echo arrives through the source like anything else.
    source
        .submit(Command::SayToCarl("fix the belt numbers".into()))
        .unwrap();
    assert_eq!(
        source.sent,
        vec![Command::SayToCarl("fix the belt numbers".into())],
        "trimmed, and sent as a message"
    );
}

#[test]
fn an_empty_message_is_not_sent() {
    let mut a = app();
    let before = a.snapshot.conversation.len();
    a.draft = "   ".into();
    a.send_draft();
    assert_eq!(a.snapshot.conversation.len(), before);
    assert!(a.notice.is_none(), "and nothing is reported either way");
}

/// An objective is a different act from a message and goes out as one.
#[test]
fn an_objective_is_its_own_command() {
    let mut source = MockPanelDataSource::new();
    source
        .submit(Command::SetObjective("make the planner correct".into()))
        .unwrap();
    assert_eq!(
        source.sent,
        vec![Command::SetObjective("make the planner correct".into())]
    );
}

/// Nothing may be sent while the link is down, and JJ has to be told rather than left to
/// assume it went.
#[test]
fn a_command_sent_while_disconnected_is_refused_out_loud() {
    let mut a = app();
    a.apply(PanelEvent::LinkChanged(Link::Disconnected {
        why: "backend closed".into(),
    }));

    a.draft = "are you there".into();
    a.send_draft();

    let (text, ok) = a.notice.clone().expect("a notice");
    assert!(!ok, "it did not go");
    assert!(text.contains("not sent"), "{text}");
}

#[test]
fn selecting_an_agent_opens_it() {
    let mut a = app();
    assert!(a.agent.is_none());
    a.select_agent("mason");
    assert_eq!(a.agent.as_deref(), Some("mason"));
}

/// An intervention written for one agent must never be sent to another because a selection
/// changed under it.
#[test]
fn moving_to_another_agent_drops_a_half_written_intervention() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::ChangeInstruction);
    a.intervening.as_mut().unwrap().body = "stop and do the smelting task".into();

    a.select_agent("mason");
    assert!(
        a.intervening.is_none(),
        "the instruction written for nora must not follow the selection"
    );
}

/// The forceful kinds are confirmed before they go. The first press asks, the second sends.
#[test]
fn a_forceful_intervention_is_confirmed_before_it_is_sent() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::StopTask);
    a.intervening.as_mut().unwrap().body = "wrong task, my mistake".into();

    assert!(!a.send_intervention(), "the first press only asks");
    assert!(a.intervening.as_ref().unwrap().confirming);
    let warning = a.intervention_warning().expect("a warning");
    assert!(warning.contains("nora"), "{warning}");

    assert!(a.send_intervention(), "the second press sends");
    assert!(a.intervening.is_none(), "and the form is cleared");
}

/// A message is not an intervention in what somebody is doing, so it goes at once.
#[test]
fn a_direct_message_needs_no_confirmation() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::Message);
    a.intervening.as_mut().unwrap().body = "good work on the belts".into();

    assert!(a.send_intervention(), "sent on the first press");
}

/// An intervention with nothing written is not an intervention.
#[test]
fn an_empty_intervention_is_refused() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::StopTask);
    assert!(!a.intervention_ready());
    assert!(!a.send_intervention());
    let (text, ok) = a.notice.clone().expect("a notice");
    assert!(!ok);
    assert!(text.contains("say what you want done"), "{text}");
}

/// What JJ is shown in the confirmation and what goes out are built by the same code, so they
/// cannot describe different things.
#[test]
fn the_command_sent_is_the_one_that_was_confirmed() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::ReplaceTask);
    a.intervening.as_mut().unwrap().body = "  do the smelting ratios instead  ".into();

    let pending = a.pending_intervention().expect("a pending intervention");
    assert_eq!(pending.agent, "nora");
    assert_eq!(pending.kind, InterventionKind::ReplaceTask);
    assert_eq!(pending.body, "do the smelting ratios instead", "trimmed");
}

/// Changing which kind of intervention keeps the words, since retyping them is the fastest way
/// to make somebody avoid the safe path.
#[test]
fn changing_the_kind_keeps_what_was_typed() {
    let mut a = app();
    a.select_agent("nora");
    a.begin_intervention(InterventionKind::Message);
    a.intervening.as_mut().unwrap().body = "the express rate is wrong".into();

    a.begin_intervention(InterventionKind::ChangeInstruction);
    assert_eq!(
        a.intervening.as_ref().unwrap().body,
        "the express rate is wrong"
    );
    assert!(
        !a.intervening.as_ref().unwrap().confirming,
        "and asks again"
    );
}

/// The workspace is opened from something and closed back to nothing.
#[test]
fn the_contextual_workspace_opens_and_closes() {
    let mut a = app();
    assert!(a.workspace.is_none());

    a.open_workspace(WorkspaceRequest::File {
        path: "/home/jj/carl/src/army/org.rs".into(),
        line: Some(42),
    });
    let open = a.workspace.clone().expect("open");
    assert_eq!(open.open.title(), "org.rs");
    assert!(
        open.content.is_none(),
        "the panel draws the container and never invents its contents"
    );

    a.open_workspace(WorkspaceRequest::Close);
    assert!(a.workspace.is_none());
}

/// A diagnostic changing must land live and replace the row rather than adding a second one.
#[test]
fn a_diagnostic_changes_live_in_place() {
    let mut a = app();
    let before = a.snapshot.diagnostics.len();

    let cpu = Diagnostic::new(
        "system.cpu",
        Health::Degraded,
        "load high",
        crate::model::Kind::Sampled,
    )
    .measured(500);
    a.apply(PanelEvent::DiagnosticChanged(Box::new(cpu)));

    assert_eq!(
        a.snapshot.diagnostics.len(),
        before,
        "replaced, not appended"
    );
    let row = a
        .snapshot
        .diagnostics
        .iter()
        .find(|d| d.component == "system.cpu")
        .unwrap();
    assert_eq!(row.health, Health::Degraded);
}

/// A milestone lands on the project it belongs to and nowhere else.
#[test]
fn a_milestone_arrives_live_on_its_own_project() {
    let mut a = app();
    let before = a.snapshot.project("jjtorio").unwrap().milestones.len();

    a.apply(PanelEvent::MilestoneReached {
        project: "jjtorio".into(),
        milestone: Box::new(Milestone {
            id: "m1".into(),
            project: carl::ProjectId::new("jjtorio").unwrap(),
            at: 999,
            title: "Belt figures verified".into(),
            detail: None,
            evidence: None,
            achievement: carl::providers::projects::Achievement::FeatureWorks,
            source: carl::providers::projects::Source::Jj,
        }),
    });

    assert_eq!(
        a.snapshot.project("jjtorio").unwrap().milestones.len(),
        before + 1
    );
    assert_eq!(
        a.snapshot
            .project("command panel")
            .unwrap()
            .milestones
            .len(),
        0,
        "and not on anybody else"
    );
}

/// The scripted timeline has to actually produce the transitions it promises, or the mock is
/// a still picture and the live behaviour was never exercised.
///
/// Driven by winding the mock's own clock forward rather than by sleeping, so the whole minute
/// of script is checked in no time at all.
#[test]
fn the_mock_timeline_drives_every_transition_it_promises() {
    let mut source = MockPanelDataSource::new();
    let mut a = App::new(Box::new(MockPanelDataSource::new()));

    let opening = a.snapshot.agent("nora").unwrap().status;
    assert_eq!(opening, AgentStatus::Idle);

    let mut seen_working = false;
    let mut seen_review = false;
    let mut seen_blocked = false;
    let mut seen_disconnect = false;
    let mut seen_reconnect = false;
    let mut seen_decision = false;
    let mut seen_milestone = false;

    for _ in 0..70 {
        source.advance(Duration::from_secs(1));
        for event in source.poll() {
            if let PanelEvent::AgentChanged(v) = &event {
                match v.status {
                    AgentStatus::Working => seen_working = true,
                    AgentStatus::AwaitingReview => seen_review = true,
                    AgentStatus::Blocked => seen_blocked = true,
                    _ => {}
                }
            }
            if let PanelEvent::LinkChanged(l) = &event {
                match l {
                    Link::Disconnected { .. } => seen_disconnect = true,
                    Link::Live if seen_disconnect => seen_reconnect = true,
                    _ => {}
                }
            }
            if matches!(event, PanelEvent::DecisionRaised(_)) {
                seen_decision = true;
            }
            if matches!(event, PanelEvent::MilestoneReached { .. }) {
                seen_milestone = true;
            }
            a.apply(event);
        }
    }

    assert!(seen_working, "a worker never started");
    assert!(seen_review, "nothing ever went for review");
    assert!(seen_blocked, "a blocker never appeared");
    assert!(seen_disconnect, "the link never dropped");
    assert!(seen_reconnect, "the link never came back");
    assert!(seen_decision, "carl never needed jj");
    assert!(seen_milestone, "no milestone was ever reached");
}

#[test]
fn a_carl_answer_marks_the_conversation_as_wanting_the_bottom() {
    let mut a = app();
    a.conversation_at_end = false;
    a.apply(PanelEvent::CarlSaid {
        text: "here".into(),
        streaming: false,
    });
    assert!(a.conversation_at_end, "new words pull the view to them");
}
