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
