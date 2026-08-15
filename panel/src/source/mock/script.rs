//! The timeline itself, on a clock, in the order it happens.
//!
//! Kept apart from the source machinery so the beats can be read as a story. Everything the
//! panel must handle well is here and nothing is random, so the same second of two runs shows
//! the same thing and a wrong colour can be looked at twice.

use std::time::Duration;

use carl::army::event::{Event, Record};
use carl::army::task::TaskId;

use super::EPOCH;
use crate::command::{Intervention, InterventionKind};
use carl::ProjectId;
use carl::providers::health::{Kind, Metric, Reading};
use carl::providers::projects::{Achievement, Project, ProjectView, Source, Status};

use crate::model::{
    AgentStatus, AgentView, Decision, Delegation, Diagnostic, Health, Link, Milestone, ProcessState,
};
use crate::source::PanelEvent;

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// A worker overlay at a given state, so the beats below stay readable.
fn nora(status: AgentStatus, activity: &str, at: u64) -> Box<AgentView> {
    let mut v = AgentView::unknown("nora");
    v.department = Some("coding".into());
    v.sub_department = Some("factorio".into());
    v.status = status;
    v.model = Some("claude-opus-5".into());
    v.process = Some(match status {
        AgentStatus::Working | AgentStatus::AwaitingReview => ProcessState::Running,
        _ => ProcessState::Stopped,
    });
    v.worktree = Some("/home/jj_tmc/Projects/jjtorio-belts".into());
    v.branch = Some("belt-throughput".into());
    v.last_activity = Some(activity.to_string());
    v.last_activity_at = Some(at);
    if status == AgentStatus::Blocked {
        v.blocker = Some(
            "run-tests.sh needs python3-pytest, which is not installed and cannot be fetched"
                .into(),
        );
    }
    Box::new(v)
}

/// Every beat, with when it fires after the panel starts.
///
/// The gaps are chosen so somebody watching can see each change land on its own rather than
/// three at once.
pub fn timeline() -> Vec<(Duration, PanelEvent)> {
    vec![
        // A worker picks the task up. The first thing anybody watching should see move.
        (
            secs(4),
            PanelEvent::AgentChanged(nora(
                AgentStatus::Working,
                "reading belts.py and the test suite",
                EPOCH + 120,
            )),
        ),
        (
            secs(7),
            PanelEvent::DiagnosticChanged(Box::new(one_diagnostic(
                "army.agent-processes",
                "army",
                Health::Healthy,
                "2 of 5 agent processes running",
                &[("running", "2"), ("stopped", "3")],
                Kind::EventDriven,
                EPOCH + 130,
            ))),
        ),
        // The work goes in for review.
        (
            secs(12),
            PanelEvent::AgentChanged(nora(
                AgentStatus::AwaitingReview,
                "submitted the express rate fix, 6 tests pass",
                EPOCH + 200,
            )),
        ),
        (
            secs(13),
            PanelEvent::Recorded(Box::new(record(
                3,
                EPOCH + 200,
                "nora",
                Event::Submitted {
                    task: TaskId::quoted("task-belt"),
                    attempt: 1,
                    words: 176,
                },
            ))),
        ),
        // A blocker, which is the state the eye must be pulled to.
        (
            secs(18),
            PanelEvent::AgentChanged(nora(
                AgentStatus::Blocked,
                "cannot run the wider suite, a dependency is missing",
                EPOCH + 260,
            )),
        ),
        (
            secs(19),
            PanelEvent::DiagnosticChanged(Box::new(one_diagnostic(
                "army.blockers",
                "army",
                Health::Blocked,
                "1 worker blocked on a missing dependency",
                &[("blocked", "1"), ("agent", "nora")],
                Kind::EventDriven,
                EPOCH + 260,
            ))),
        ),
        // Carl needs JJ.
        (
            secs(23),
            PanelEvent::DecisionRaised(Box::new(Decision {
                id: "d-pytest".into(),
                asked_at: EPOCH + 280,
                question: "Nora needs python3-pytest to run the wider suite. It needs \
                           installing, which I cannot do."
                    .into(),
                detail: Some(
                    "The express belt fix is verified by the project's own runner already. The \
                     wider suite is what needs the package. I can accept the narrower proof, or \
                     hold the task until you install it."
                        .into(),
                ),
                options: vec![
                    "Accept the narrower proof".into(),
                    "Hold until I install it".into(),
                ],
            })),
        ),
        // The link goes, which must be obvious and must not be faked through.
        (
            secs(30),
            PanelEvent::LinkChanged(Link::Disconnected {
                why: "backend closed the connection".into(),
            }),
        ),
        (
            secs(33),
            PanelEvent::LinkChanged(Link::Connecting { attempt: 1 }),
        ),
        (
            secs(37),
            PanelEvent::LinkChanged(Link::Connecting { attempt: 2 }),
        ),
        (secs(41), PanelEvent::LinkChanged(Link::Live)),
        // Back, and moving again.
        (
            secs(44),
            PanelEvent::AgentChanged(nora(
                AgentStatus::Working,
                "narrower proof accepted, tidying the fix",
                EPOCH + 400,
            )),
        ),
        (
            secs(48),
            PanelEvent::DiagnosticChanged(Box::new(one_diagnostic(
                "system.cpu",
                "system",
                Health::Degraded,
                "load high while four agents build",
                &[("load 1m", "7.8"), ("cores", "8")],
                Kind::Sampled,
                EPOCH + 420,
            ))),
        ),
        // Something that actually mattered.
        (
            secs(53),
            PanelEvent::MilestoneReached {
                project: "jjtorio".into(),
                milestone: Box::new(milestone(
                    EPOCH + 450,
                    "Belt throughput figures verified against the game data",
                    Some("express 45/s, fast 30/s, transport 15/s"),
                )),
            },
        ),
        (
            secs(58),
            PanelEvent::Delegated(Box::new(Delegation {
                at: EPOCH + 470,
                from: "mason".into(),
                to: "nora".into(),
                goal: "Check the smelting ratios against the same source".into(),
                task: None,
            })),
        ),
    ]
}

/// Carl answering, in pieces, so the streaming area has something to stream.
///
/// The last piece is the only one marked finished, which is what tells the panel to stop
/// showing a caret and treat the turn as said.
pub fn carl_reply(to: &str) -> Vec<(String, bool)> {
    let opening = if to.len() < 24 {
        "Understood."
    } else {
        "Understood, and I have passed it down rather than doing it myself."
    };
    vec![
        (opening.to_string(), true),
        (" Adrian has it".to_string(), true),
        (
            ", and he will route the Factorio part to Mason".to_string(),
            true,
        ),
        (
            ". I will come back when it is verified rather than when it is claimed.".to_string(),
            false,
        ),
    ]
}

/// What an intervention looks like in the record once the backend has taken it.
pub fn intervention_record(i: &Intervention) -> Record {
    let what = match i.kind {
        InterventionKind::Message => format!("jj messaged {} directly", i.agent),
        InterventionKind::ChangeInstruction => format!("jj changed {}'s instruction", i.agent),
        InterventionKind::StopTask => format!("jj stopped {}'s task", i.agent),
        InterventionKind::ReplaceTask => format!("jj replaced {}'s task", i.agent),
    };
    record(
        99,
        EPOCH + 500,
        "jj",
        Event::Decided {
            task: None,
            what: format!("{what}. {}", i.body),
        },
    )
}

fn record(seq: u64, at: u64, actor: &str, event: Event) -> Record {
    Record {
        seq,
        at,
        actor: actor.to_string(),
        event,
    }
}

#[allow(clippy::too_many_arguments)]
fn one_diagnostic(
    component: &str,
    _group: &str,
    health: Health,
    summary: &str,
    metrics: &[(&str, &str)],
    kind: Kind,
    at: u64,
) -> Diagnostic {
    let mut d = Diagnostic::new(component, health, summary, kind);
    for (name, value) in metrics {
        // Text readings, because these are labels rather than measurements and pretending
        // otherwise would put a number on screen nobody measured.
        d = d.with(Metric::new(name, Reading::Text((*value).to_string()), ""));
    }
    // Only a sample carries a moment. Army state is true until something changes it.
    if kind == Kind::Sampled {
        d = d.measured(at);
    }
    d
}

/// The opening diagnostics board.
///
/// Includes components nothing has measured, on purpose, because the panel has to draw an
/// honest gap and that cannot be checked if every row has a number in it.
pub fn diagnostics(now: u64) -> Vec<Diagnostic> {
    vec![
        one_diagnostic(
            "army.carl",
            "army",
            Health::Healthy,
            "chief process up, 2 turns in the last hour",
            &[("process", "running"), ("model", "claude-opus-5")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "army.agent-processes",
            "army",
            Health::Healthy,
            "1 of 5 agent processes running",
            &[("running", "1"), ("stopped", "4")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "army.tasks",
            "army",
            Health::Healthy,
            "1 task in hand, 0 awaiting review",
            &[("in hand", "1"), ("submitted", "0"), ("accepted", "3")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "army.blockers",
            "army",
            Health::Healthy,
            "nothing blocked",
            &[("blocked", "0")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "army.journal",
            "army",
            Health::Healthy,
            "events.jsonl readable, 17 records, numbering continuous",
            &[("records", "17"), ("last seq", "17")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "army.backend",
            "army",
            Health::Degraded,
            "mock source, no live backend attached",
            &[("source", "mock")],
            Kind::EventDriven,
            now,
        ),
        one_diagnostic(
            "system.cpu",
            "system",
            Health::Healthy,
            "load 2.1 across 8 cores",
            &[("load 1m", "2.1"), ("cores", "8")],
            Kind::Sampled,
            now,
        ),
        one_diagnostic(
            "system.memory",
            "system",
            Health::Healthy,
            "9.4 GB of 31 GB in use",
            &[("used", "9.4 GB"), ("total", "31 GB")],
            Kind::Sampled,
            now,
        ),
        one_diagnostic(
            "system.swap",
            "system",
            Health::Healthy,
            "nothing swapped",
            &[("used", "0 B"), ("total", "2.0 GB")],
            Kind::Sampled,
            now,
        ),
        one_diagnostic(
            "system.disk",
            "system",
            Health::Degraded,
            "root filesystem 86 percent full",
            &[("used", "402 GB"), ("total", "468 GB")],
            Kind::Sampled,
            now,
        ),
        // Deliberately unmeasured, both ways round, because the two are different facts and
        // the screen has to draw them differently.
        //
        // The GPU was looked at and there is no card, so it keeps its moment and names what it
        // could not read. The sensor has never been read at all, so it has no moment.
        Diagnostic::new(
            "system.gpu",
            Health::Unknown,
            "no NVIDIA card on this machine",
            Kind::Sampled,
        )
        .with(Metric::new("vram", Reading::Unknown, "MiB"))
        .measured(now),
        Diagnostic::new(
            "system.temperature",
            Health::Unknown,
            "no sensor has been read",
            Kind::Sampled,
        ),
    ]
}

/// A project id from a name known to be valid, since these are fixtures.
fn pid(name: &str) -> ProjectId {
    ProjectId::new(name).expect("the fixture ids are well formed")
}

/// A milestone in the canonical shape, with the fields the record carries.
fn milestone(at: u64, title: &str, detail: Option<&str>) -> Milestone {
    Milestone {
        id: format!("m-{at}"),
        project: pid("jjtorio"),
        at,
        title: title.to_string(),
        detail: detail.map(str::to_string),
        evidence: Some("run-tests.sh".into()),
        achievement: Achievement::FeatureWorks,
        source: Source::Lead("mason".into()),
    }
}

/// The opening projects board.
pub fn projects(now: u64) -> Vec<ProjectView> {
    let mut jjtorio = Project::new(
        pid("jjtorio"),
        "jjtorio",
        "A Factorio mod and the planning tools around it, so JJ can size a build from real \
         numbers instead of guessing.",
    );
    jjtorio.status = Status::Active;
    jjtorio.phase = "correcting the belt figures against the game data".into();
    jjtorio.department = Some("coding".into());
    jjtorio.next_objective = Some("Smelting ratios, same source, same proof".into());

    let mut panel = Project::new(
        pid("command-panel"),
        "command panel",
        "A fullscreen operations interface for the army, so JJ can see what everyone is doing \
         without reading a journal file.",
    );
    panel.status = Status::Active;
    panel.phase = "shell and four tabs against a live backend".into();
    panel.department = Some("coding".into());
    panel.blockers = vec!["The OpenGL backend draws glyphs black on this machine".into()];

    let mut aos = Project::new(
        pid("aos"),
        "aos",
        "The supervisor that will run the army as processes rather than as one program.",
    );
    aos.status = Status::Paused;
    aos.phase = "planned".into();

    vec![
        ProjectView {
            project: jjtorio,
            // Newest first, which is the order the provider keeps and the pane draws.
            milestones: vec![
                milestone(
                    now - 3_200,
                    "Express belt rate corrected to 45 per second",
                    Some("was 40, proven by the project's own runner"),
                ),
                milestone(now - 8_600, "Planner runs its own test suite", None),
            ],
            active_tasks: vec![carl::army::task::TaskId::quoted("t-belt-throughput")],
            active_agents: vec!["nora".into()],
            // One line of the milestone file that would not parse, so the pane has something
            // to be honest about rather than only ever showing a clean list.
            milestone_gaps: 1,
        },
        ProjectView {
            project: panel,
            milestones: Vec::new(),
            active_tasks: Vec::new(),
            active_agents: Vec::new(),
            milestone_gaps: 0,
        },
        ProjectView {
            project: aos,
            milestones: Vec::new(),
            active_tasks: Vec::new(),
            active_agents: Vec::new(),
            milestone_gaps: 0,
        },
    ]
}
