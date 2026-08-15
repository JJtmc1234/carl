//! The providers as the panel sees them, over the real socket.
//!
//! The questions here are all about what survives the trip. The collectors are careful to keep
//! two distinctions that a careless wire format would flatten: a reading that could not be taken
//! is not zero, and a fact that is true until something changes it is not a fact measured at an
//! instant. Both of those are easy to lose in a `f64` and a `u64`, and neither would look wrong
//! on screen once lost, which is why they are checked here rather than trusted.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carl::ProjectId;
use carl::army::event::{Event, Journal};
use carl::army::personnel::found;
use carl::army::task::{Status, Task, Verification};
use carl::panel::client::PanelClient;
use carl::panel::listen;
use carl::providers::health::{Health, Kind, Metric, Reading};
use carl::providers::projects::Projects;
use carl::providers::projects::model::Project;

struct Backend {
    home: PathBuf,
    child: Option<std::process::Child>,
}

impl Backend {
    fn start(home: &Path) -> Self {
        let mut me = Self {
            home: home.to_path_buf(),
            child: None,
        };
        me.up();
        me
    }

    fn up(&mut self) {
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_carl"))
            .arg("--home")
            .arg(&self.home)
            .arg("panel")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("starting carl panel");
        self.child = Some(child);
        for _ in 0..400 {
            if PanelClient::connect(&self.socket()).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend never came up");
    }

    fn down(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        for _ in 0..400 {
            if PanelClient::connect(&self.socket()).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend never went away");
    }

    fn socket(&self) -> PathBuf {
        listen::socket_path(&self.home)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

fn jjtorio() -> ProjectId {
    ProjectId::new("jjtorio").unwrap()
}

/// A home with an army, a project, and one task linked to it.
fn a_working_home(dir: &Path) -> Task {
    let people = found(dir, 1).unwrap();

    let projects = Projects::open(dir);
    projects
        .save(&Project::new(
            jjtorio(),
            "JJtorio",
            "make the mod start faster",
        ))
        .unwrap();

    let t = Task::assign(
        "mason",
        "nora",
        "cache the prototype lookup",
        verification(),
    )
    .unwrap()
    .for_project(jjtorio());

    let mut journal = Journal::open(people.journal_path()).unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: None,
                must: t.verification.must.clone(),
                project: t.project.clone(),
            },
        )
        .unwrap();
    journal
        .append(
            "nora",
            Event::moved(&t.id, Status::Assigned, Status::InHand),
        )
        .unwrap();
    t
}

#[test]
fn a_project_shows_the_task_and_the_agent_actually_linked_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let t = a_working_home(dir.path());
    let backend = Backend::start(dir.path());

    let snapshot = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    assert_eq!(snapshot.projects.len(), 1);
    let view = &snapshot.projects[0];
    assert_eq!(view.project.id, jjtorio());
    assert_eq!(
        view.active_tasks
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>(),
        vec![t.id.to_string()]
    );
    assert_eq!(
        view.active_agents,
        vec!["nora"],
        "derived from the linked task"
    );
    assert!(
        view.milestones.is_empty(),
        "nothing recorded one, so there are none"
    );

    // And the task carries the link back the other way.
    let task = snapshot
        .tasks
        .iter()
        .find(|x| x.id == t.id.to_string())
        .unwrap();
    assert_eq!(task.project, Some(jjtorio()));
}

/// A task nobody linked must not drift into the only project that exists.
#[test]
fn an_unlinked_task_does_not_join_a_project_over_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    let people = found(dir.path(), 1).unwrap();
    Projects::open(dir.path())
        .save(&Project::new(jjtorio(), "JJtorio", "faster"))
        .unwrap();

    let t = Task::assign("mason", "nora", "an unrelated errand", verification()).unwrap();
    let mut journal = Journal::open(people.journal_path()).unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: None,
                must: t.verification.must.clone(),
                project: None,
            },
        )
        .unwrap();

    let backend = Backend::start(dir.path());
    let snapshot = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    assert!(snapshot.projects[0].active_tasks.is_empty());
    assert!(snapshot.projects[0].active_agents.is_empty());
    assert_eq!(snapshot.tasks[0].project, None);
}

/// The distinction the collectors are careful about, checked after a JSON round trip.
#[test]
fn an_unknown_reading_is_still_unknown_after_crossing_the_socket() {
    let unknown = Metric::unknown("gpu.temperature", "C");
    let known = Metric::new("cpu.load", Reading::Float(0.5), "%");

    for m in [&unknown, &known] {
        let text = serde_json::to_string(m).unwrap();
        let back: Metric = serde_json::from_str(&text).unwrap();
        assert_eq!(&back, m, "{text}");
    }

    // The part that matters. Unknown must not arrive as a number anybody could act on.
    assert_eq!(unknown.value, Reading::Unknown);
    assert_eq!(unknown.value.as_f64(), None, "and never reads as zero");
    assert_ne!(unknown.value, Reading::Float(0.0));
    assert!(!unknown.value.is_known());
    assert_eq!(unknown.unit, "C", "what it would have been is still useful");
}

/// A sampled reading nobody could take still says when the attempt was made.
#[test]
fn a_sampled_unknown_keeps_its_measured_at() {
    let d = carl::providers::health::Diagnostic::new(
        "system.gpu",
        Health::Unknown,
        "no supported GPU found",
        Kind::Sampled,
    )
    .with(Metric::unknown("gpu.temperature", "C"))
    .measured(1_755_200_000);

    let back: carl::providers::health::Diagnostic =
        serde_json::from_str(&serde_json::to_string(&d).unwrap()).unwrap();

    assert_eq!(back.measured_at, Some(1_755_200_000), "when we looked");
    assert_eq!(back.health, Health::Unknown, "and that we found nothing");
    assert_eq!(back.metrics[0].value, Reading::Unknown);
    assert_eq!(back.kind, Kind::Sampled);
}

#[test]
fn the_two_kinds_of_diagnostic_stay_apart_on_the_wire() {
    let dir = tempfile::tempdir().unwrap();
    found(dir.path(), 1).unwrap();
    let backend = Backend::start(dir.path());

    let snapshot = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    let sampled: Vec<_> = snapshot
        .diagnostics
        .iter()
        .filter(|d| d.kind == Kind::Sampled)
        .collect();
    let event_driven: Vec<_> = snapshot
        .diagnostics
        .iter()
        .filter(|d| d.kind == Kind::EventDriven)
        .collect();

    assert!(!sampled.is_empty(), "the machine was read");
    assert!(!event_driven.is_empty(), "and the army was folded");

    for d in &sampled {
        assert!(
            d.measured_at.is_some(),
            "a sample without a moment is a sample you cannot age: {}",
            d.component
        );
    }
    for d in &event_driven {
        assert!(
            d.measured_at.is_none(),
            "army state is true until something changes it, not true at an instant: {}",
            d.component
        );
    }
}

/// Nothing associates a pid with an agent, so nothing may claim one is running.
#[test]
fn no_agent_claims_a_process_because_a_claude_is_running_somewhere() {
    let dir = tempfile::tempdir().unwrap();
    found(dir.path(), 1).unwrap();
    let backend = Backend::start(dir.path());

    let snapshot = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    for agent in &snapshot.agents {
        assert!(
            agent.process.is_unknown(),
            "{} claims a process state nothing can establish",
            agent.name
        );
    }
}

/// A restart must rebuild the link from the record, because that is where it lives.
#[test]
fn the_project_link_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let t = a_working_home(dir.path());
    let mut backend = Backend::start(dir.path());

    let before = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    backend.down();
    backend.up();

    let after = PanelClient::connect(&backend.socket())
        .unwrap()
        .snapshot()
        .unwrap();

    assert_eq!(
        after.projects[0].active_tasks,
        before.projects[0].active_tasks
    );
    assert_eq!(after.projects[0].active_agents, vec!["nora"]);
    assert_eq!(
        after
            .tasks
            .iter()
            .find(|x| x.id == t.id.to_string())
            .unwrap()
            .project,
        Some(jjtorio()),
        "rebuilt from the journal, which is the only place it was written"
    );
}

/// A resync is replacement truth, not something to merge into what was already held.
#[test]
fn a_resynced_snapshot_replaces_provider_state_rather_than_merging_it() {
    use carl::panel::live::{LivePanel, Update};

    let dir = tempfile::tempdir().unwrap();
    a_working_home(dir.path());
    let backend = Backend::start(dir.path());

    let (mut live, first) = LivePanel::open(&backend.socket()).unwrap();
    assert_eq!(first.projects[0].active_tasks.len(), 1);

    // The record is replaced under the running panel, so the sequence it holds cannot be
    // honoured. Everything it was showing is now history.
    let people = carl::army::personnel::Personnel::open(dir.path()).unwrap();
    std::fs::write(people.journal_path(), "").unwrap();
    let mut journal = Journal::open(people.journal_path()).unwrap();
    journal
        .append(
            "mason",
            Event::Decided {
                task: None,
                what: "starting again".into(),
            },
        )
        .unwrap();

    let fresh = loop {
        match live.next_update() {
            Update::Resynced(s) => break s,
            Update::Health(_) | Update::Telemetry { .. } => continue,
            Update::Event(e) => panic!("nothing was resumable: {}", e.seq),
        }
    };

    // The replacement is built from the record as it now is. The task that was linked is gone
    // from it, and nothing carried the old project link forward.
    assert!(
        fresh.tasks.is_empty(),
        "the record no longer holds that task"
    );
    assert!(
        fresh.projects[0].active_tasks.is_empty(),
        "so the project has no active work, rather than the work it used to have"
    );
    assert!(
        fresh.projects[0].active_agents.is_empty(),
        "and nobody is shown working on it"
    );
    assert_eq!(live.last_seq(), fresh.seq);
}
