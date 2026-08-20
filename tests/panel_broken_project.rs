//! A project that will not load must still appear on the panel.
//!
//! The store's own doc states the rule: a project that vanishes from a panel is worse than one
//! that shows up broken, because a vanished project looks like one that was never created. Bug
//! 12 was that rule being inverted by the code underneath it. `Projects::list` used `?` on the
//! per entry work, so the first unreadable folder ended the whole walk, and `facts.rs` turned
//! that one error into an empty vector with `unwrap_or_default`.
//!
//! This lives out here rather than next to the store because the store was only half the bug.
//! The other half was the panel, and a guard that stops at the store would have passed while
//! the screen was still empty.

use std::path::Path;

use carl::ProjectId;
use carl::army::personnel::found;
use carl::panel::Facts;
use carl::providers::diagnostics::Diagnostics;
use carl::providers::projects::{Project, Projects};

fn a_home(dir: &Path) -> Projects {
    found(dir, 1).unwrap();
    Projects::open(dir)
}

fn save(projects: &Projects, name: &str) {
    projects
        .save(&Project::new(
            ProjectId::new(name).unwrap(),
            name,
            "a real project",
        ))
        .unwrap();
}

/// Two good projects and one broken one is three rows, not zero.
#[test]
fn one_broken_project_does_not_empty_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let projects = a_home(dir.path());
    let mut diagnostics = Diagnostics::new(dir.path());

    for name in ["aos", "carl", "jjtorio"] {
        save(&projects, name);
    }
    std::fs::write(dir.path().join("projects/carl/project.json"), "{ not json").unwrap();

    let facts = Facts::gather_at(&mut diagnostics, &projects, &[], 1_755_200_000);

    let ids: Vec<_> = facts
        .projects
        .iter()
        .map(|v| v.project.id.to_string())
        .collect();
    assert_eq!(ids, ["aos", "carl", "jjtorio"], "the panel lost a project");

    let broken = facts
        .projects
        .iter()
        .find(|v| v.project.id.as_str() == "carl")
        .unwrap();
    assert!(
        broken.project.name.contains("unreadable"),
        "shown as healthy: {:?}",
        broken.project
    );
    assert!(
        broken
            .project
            .blockers
            .iter()
            .any(|b| b.contains("project.json")),
        "shown broken but with no reason: {:?}",
        broken.project
    );
}

/// A folder that could never be a project id must not take the real ones with it.
#[test]
fn a_stray_folder_does_not_empty_the_panel() {
    let dir = tempfile::tempdir().unwrap();
    let projects = a_home(dir.path());
    let mut diagnostics = Diagnostics::new(dir.path());

    save(&projects, "aos");
    // Capitals are the realistic way in. Somebody makes `Notes` by hand and every project on
    // the screen disappears, which is a long way from what they did.
    std::fs::create_dir_all(dir.path().join("projects").join("Notes")).unwrap();

    let facts = Facts::gather_at(&mut diagnostics, &projects, &[], 1_755_200_000);
    let ids: Vec<_> = facts
        .projects
        .iter()
        .map(|v| v.project.id.to_string())
        .collect();
    assert_eq!(ids, ["aos"], "a folder called Notes emptied the panel");
}
