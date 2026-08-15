//! The provider contract, as one runnable oracle.
//!
//! Written for whoever integrates these providers rather than for me. Everything asserted here
//! is a promise the provider layer makes to its caller, so if one of these fails after a merge
//! the caller's assumptions have been broken, not merely an internal detail.
//!
//! `cargo test --test providers`
//!
//! No user interface, no transport, no army is founded and no project is invented. Every check
//! runs against a temporary home so it says the same thing on any machine.

use carl::providers::diagnostics::Intervals;
use carl::providers::health::{Kind, Reading};
use carl::providers::projects::{Achievement, NewMilestone, Project, ProjectId, Projects, Source};
use carl::providers::{Diagnostics, Health, Snapshot};

fn home() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/// One snapshot, taken at a fixed instant so nothing here depends on timing.
fn snapshot_of(home: &std::path::Path) -> Snapshot {
    Diagnostics::new(home).snapshot_at(1_000_000)
}

/// The whole contract in one test, for a caller that wants a single thing to run.
#[test]
fn the_provider_contract_holds() {
    let d = home();
    let taken = snapshot_of(d.path());

    // 1. Every component id is unique. A duplicate would have a panel draw one row over
    //    another, and which one survived would depend on iteration order.
    assert!(
        taken.duplicate_components().is_empty(),
        "duplicate component ids: {:?}",
        taken.duplicate_components()
    );

    // 2. Every component belongs to one of exactly two families.
    for diagnostic in taken.all() {
        let group = diagnostic.group();
        assert!(
            group == "army" || group == "system",
            "{} is in group {group:?}, which is neither",
            diagnostic.component
        );
        assert!(
            diagnostic.component.starts_with("army.")
                || diagnostic.component.starts_with("system."),
            "{} does not follow the id convention",
            diagnostic.component
        );
    }

    // 3. Sampled readings say when they were taken. Telemetry without an age is telemetry
    //    that will be drawn as though it were current.
    for diagnostic in &taken.machine {
        assert_eq!(diagnostic.kind, Kind::Sampled, "{}", diagnostic.component);
        assert_eq!(
            diagnostic.measured_at,
            Some(1_000_000),
            "{} did not record when it was read",
            diagnostic.component
        );
    }

    // 4. Event driven readings do not, because they are true until something changes them.
    for diagnostic in &taken.army {
        assert_eq!(
            diagnostic.kind,
            Kind::EventDriven,
            "{}",
            diagnostic.component
        );
        assert_eq!(
            diagnostic.measured_at, None,
            "{} carries an age it should not have",
            diagnostic.component
        );
    }

    // 5. Unknown is never zero, and zero is never unknown.
    for diagnostic in taken.all() {
        for metric in &diagnostic.metrics {
            match &metric.value {
                Reading::Unknown => assert_eq!(
                    metric.rendered(),
                    "unknown",
                    "{}/{} rendered an unmeasurable value as something else",
                    diagnostic.component,
                    metric.name
                ),
                known => {
                    assert!(
                        known.is_known(),
                        "{}/{} claims to be known and is not",
                        diagnostic.component,
                        metric.name
                    );
                    assert_ne!(
                        metric.rendered(),
                        "unknown",
                        "{}/{} rendered a real measurement as unknown",
                        diagnostic.component,
                        metric.name
                    );
                }
            }
        }
    }

    // 6. Overall health is the worst of everything, and asking twice gives the same answer.
    let overall = taken.overall();
    assert_eq!(
        overall,
        taken.overall(),
        "overall health is not a pure function"
    );
    assert_eq!(
        overall,
        Health::worst(taken.all().into_iter().map(|d| d.health)),
        "overall health disagrees with its own parts"
    );
}

/// Reading a machine with no army must not create one. A panel that founds an army by being
/// opened would change the thing it is supposed to be observing.
#[test]
fn no_read_operation_founds_an_army() {
    let d = home();
    let mut diagnostics = Diagnostics::new(d.path());

    diagnostics.snapshot_at(1_000);
    diagnostics.army_at(1_000);
    diagnostics.machine_at(1_000);
    diagnostics.records();
    diagnostics.probes_at(1_000);
    diagnostics.snapshot_at(2_000);

    assert!(
        !d.path().join("army").exists(),
        "the provider founded an army by looking at one"
    );
    assert!(!d.path().join("run").exists(), "and it wrote a journal too");

    // And it says so honestly rather than reporting an army of nobody.
    let taken = snapshot_of(d.path());
    let personnel = taken
        .find("army.personnel")
        .expect("the row is still produced");
    assert_eq!(personnel.health, Health::Unknown);
    assert_eq!(
        personnel.metrics[0].rendered(),
        "unknown",
        "an unfounded army must not report zero agents"
    );
}

/// The project store and the diagnostics do not touch each other.
#[test]
fn the_project_store_is_independent_of_diagnostics() {
    let d = home();

    let before = snapshot_of(d.path());
    let before_ids: Vec<String> = before.components().iter().map(|c| c.to_string()).collect();

    // Create a project and record something on it.
    let projects = Projects::open(d.path());
    let id = ProjectId::new("jjtorio").unwrap();
    projects
        .save(&Project::new(id.clone(), "JJtorio", "A mod that works"))
        .unwrap();
    projects
        .record(NewMilestone {
            project: id.clone(),
            at: 100,
            title: "the belts balance".into(),
            detail: None,
            evidence: Some("commit abc123".into()),
            achievement: Achievement::FeatureWorks,
            source: Source::Jj,
        })
        .unwrap();

    // Diagnostics are unmoved by it.
    let after = snapshot_of(d.path());
    let after_ids: Vec<String> = after.components().iter().map(|c| c.to_string()).collect();
    assert_eq!(
        before_ids, after_ids,
        "a project changed the diagnostics board"
    );

    // And reading diagnostics did not disturb the project store.
    assert_eq!(projects.list().unwrap().len(), 1);
    assert_eq!(projects.milestones(&id).unwrap().len(), 1);
    assert_eq!(projects.milestone_gaps(&id).unwrap(), 0);
}

/// Reading the project store on a machine with no projects creates nothing.
#[test]
fn no_read_operation_creates_a_project_store() {
    let d = home();
    let projects = Projects::open(d.path());

    assert!(projects.list().unwrap().is_empty());
    let id = ProjectId::new("nothing-here").unwrap();
    assert!(projects.get(&id).unwrap().is_none());
    assert!(projects.view(&id).unwrap().is_none());
    assert!(projects.milestones(&id).unwrap().is_empty());
    assert!(projects.suggestions(&id).unwrap().is_empty());

    assert!(
        !d.path().join("projects").exists(),
        "reading created the store"
    );
}

/// The rate limit, which is the property that decides whether this is safe in a render loop.
#[test]
fn a_render_loop_does_not_multiply_child_processes() {
    let d = home();
    let mut diagnostics = Diagnostics::new(d.path());

    for _ in 0..240 {
        diagnostics.snapshot_at(5_000);
    }

    assert_eq!(
        diagnostics.samples_taken(),
        1,
        "the machine sampler ran more than once at a single instant"
    );
    assert_eq!(
        diagnostics.probes_taken(),
        1,
        "the service probe ran more than once at a single instant"
    );
}

/// Both boards are populated on a real machine, so the oracle is checking something.
#[test]
fn both_boards_have_components() {
    let d = home();
    let taken = snapshot_of(d.path());

    let army = taken.army.len();
    let machine = taken.machine.len();
    assert!(army >= 5, "only {army} army components");
    assert!(machine >= 5, "only {machine} machine components");
    assert_eq!(taken.all().len(), army + machine);
}

// ---- read only audit ----

/// Every path and size under `root`, so two moments can be compared exactly.
fn tree(root: &std::path::Path) -> Vec<(String, u64)> {
    fn walk(at: &std::path::Path, base: &std::path::Path, into: &mut Vec<(String, u64)>) {
        let Ok(entries) = std::fs::read_dir(at) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            match entry.metadata() {
                Ok(meta) if meta.is_dir() => {
                    into.push((format!("{name}/"), 0));
                    walk(&path, base, into);
                }
                Ok(meta) => into.push((name, meta.len())),
                Err(_) => into.push((name, 0)),
            }
        }
    }

    let mut found = Vec::new();
    walk(root, root, &mut found);
    found.sort();
    found
}

/// The audit: no conceptually read only provider call may leave a mark on disk.
///
/// This is the promise a panel depends on most quietly. A panel is an observer, and an
/// observer that changes what it looks at is worse than no panel, because the change happens
/// on somebody's real machine and nobody connects it to having opened a window.
#[test]
fn no_read_only_provider_operation_writes_to_disk() {
    let d = home();
    let before = tree(d.path());
    assert!(before.is_empty(), "the fixture should start empty");

    // Diagnostics, every read path it has.
    let mut diagnostics = Diagnostics::new(d.path());
    diagnostics.snapshot();
    diagnostics.snapshot_at(1_000);
    diagnostics.army();
    diagnostics.army_at(1_000);
    diagnostics.machine();
    diagnostics.machine_at(1_000);
    diagnostics.records();
    diagnostics.probes_at(1_000);
    diagnostics.last_sampled_at();
    diagnostics.last_probed_at();
    diagnostics.home();

    // The army provider directly, including the paths that touch personnel.
    let army = carl::providers::army::Army::new(d.path());
    army.founded();
    army.records();
    army.processes(None);
    army.snapshot(None);
    army.overall(None);
    army.army_root();
    army.journal_path();

    // The project store, every read path it has.
    let projects = Projects::open(d.path());
    let id = ProjectId::new("nothing-here").unwrap();
    projects.list().unwrap();
    projects.get(&id).unwrap();
    projects.view(&id).unwrap();
    projects.milestones(&id).unwrap();
    projects.recent_milestones(&id, 5).unwrap();
    projects.milestone_gaps(&id).unwrap();
    projects.suggestions(&id).unwrap();
    projects.root();
    projects.folder(&id);

    // Workspace investigation, which is a lookup and must open nothing.
    let snapshot = diagnostics.snapshot_at(2_000);
    let workspace = carl::providers::workspace::Workspace::new();
    workspace.investigate(&snapshot, "system.memory");
    workspace.investigate(&snapshot, "army.personnel");
    workspace.investigate(&snapshot, "; touch /tmp/should-not-exist");
    workspace.investigate(&snapshot, "../../etc/passwd");

    let after = tree(d.path());
    assert_eq!(
        after, before,
        "a read only operation changed the filesystem: {after:?}"
    );
    assert!(
        !d.path().join("army").exists(),
        "the army directory appeared without anybody founding one"
    );
}

/// The two expensive clocks are independent, so a caller can tune one without the other.
#[test]
fn sample_and_probe_clocks_are_independent() {
    let d = home();
    let mut diagnostics = Diagnostics::new(d.path()).every(Intervals {
        machine_secs: 1,
        probe_secs: 10,
    });

    for second in 0..10 {
        diagnostics.snapshot_at(9_000 + second);
    }
    assert_eq!(
        diagnostics.samples_taken(),
        10,
        "the fast clock did not run"
    );
    assert_eq!(diagnostics.probes_taken(), 1, "the slow clock followed it");
}

/// Rich unknown, which is the semantics agreed with the panel.
#[test]
fn unknown_readings_keep_their_detail() {
    let d = home();
    let taken = snapshot_of(d.path());

    // An unfounded army is the reliable unknown on any fresh machine.
    let personnel = taken.find("army.personnel").expect("the row exists");
    assert_eq!(personnel.health, Health::Unknown);
    assert!(
        !personnel.metrics.is_empty(),
        "a rich unknown still names what was missing"
    );
    for metric in &personnel.metrics {
        assert_eq!(metric.rendered(), "unknown");
        assert!(!metric.value.is_known());
    }

    // The lossy view is available, and does not change what it was made from.
    let flat = personnel.flattened();
    assert!(flat.metrics.is_empty(), "the flattened view is a gap");
    assert_eq!(flat.measured_at, None);
    assert!(
        !personnel.metrics.is_empty(),
        "flattening emptied the canonical value it was made from"
    );
    assert_eq!(
        personnel.metrics[0].rendered(),
        "unknown",
        "and the canonical metric still reads as unmeasurable rather than gone"
    );
}

/// A hole in a history has to be visible, or a panel shows a shorter timeline and calls it
/// complete.
#[test]
fn a_damaged_milestone_history_reports_its_gap() {
    let d = home();
    let projects = Projects::open(d.path());
    let id = ProjectId::new("jjtorio").unwrap();
    projects
        .save(&Project::new(id.clone(), "JJtorio", "A mod that works"))
        .unwrap();

    let record = |title: &str, at: u64| NewMilestone {
        project: id.clone(),
        at,
        title: title.to_string(),
        detail: None,
        evidence: None,
        achievement: Achievement::PhaseCompleted,
        source: Source::Carl,
    };
    projects.record(record("one", 1)).unwrap();
    projects.record(record("two", 2)).unwrap();

    let path = d.path().join("projects/jjtorio/milestones.jsonl");
    let whole = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, &whole[..whole.len() - 20]).unwrap();

    // The damage is counted, and the next append still lands whole.
    assert_eq!(projects.milestone_gaps(&id).unwrap(), 1);
    projects.record(record("three", 3)).unwrap();

    let reopened = Projects::open(d.path());
    let titles: Vec<String> = reopened
        .milestones(&id)
        .unwrap()
        .into_iter()
        .map(|m| m.title)
        .collect();
    assert_eq!(titles, ["one", "three"], "the damage spread");
    assert_eq!(reopened.milestone_gaps(&id).unwrap(), 1);
    assert_eq!(reopened.view(&id).unwrap().unwrap().milestone_gaps, 1);
}

/// A component id is a key. It is never run, never a path, and never anything else.
#[test]
fn workspace_investigation_is_inert() {
    let d = home();
    let snapshot = snapshot_of(d.path());
    let workspace = carl::providers::workspace::Workspace::new();

    let sentinel = std::path::Path::new("/tmp/carl-panel-oracle-should-not-exist");
    for hostile in [
        "; touch /tmp/carl-panel-oracle-should-not-exist",
        "$(touch /tmp/carl-panel-oracle-should-not-exist)",
        "`touch /tmp/carl-panel-oracle-should-not-exist`",
        "system.memory && rm -rf /",
        "../../../etc/passwd",
        "system.memory\nsystem.cpu",
        "",
    ] {
        assert_eq!(
            workspace.investigate(&snapshot, hostile),
            None,
            "{hostile:?} matched a component"
        );
    }
    assert!(!sentinel.exists(), "a component id was executed");

    // A real id still works, so the check is not simply refusing everything.
    assert!(workspace.investigate(&snapshot, "system.memory").is_some());
}
