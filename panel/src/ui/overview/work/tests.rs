//! The order the projects block is read in.

use super::*;
use crate::source::{MockPanelDataSource, PanelDataSource};
use carl::providers::projects::Project;

fn project(name: &str, status: Status, blockers: &[&str]) -> ProjectView {
    let mut p = Project::new(
        carl::ProjectId::new(name).expect("a well formed fixture id"),
        name,
        "goal",
    );
    p.status = status;
    p.blockers = blockers.iter().map(|b| (*b).to_string()).collect();
    ProjectView {
        project: p,
        milestones: Vec::new(),
        active_tasks: Vec::new(),
        active_agents: Vec::new(),
        milestone_gaps: 0,
    }
}

/// A held up project outranks a running one, and a paused one is not news at all.
#[test]
fn a_blocked_project_sorts_above_a_running_one() {
    let all = vec![
        project("done", Status::Done, &[]),
        project("paused", Status::Paused, &[]),
        project("running", Status::Active, &[]),
        project(
            "held",
            Status::Active,
            &["the renderer draws black on black"],
        ),
    ];
    let names: Vec<&str> = ordered(&all)
        .iter()
        .map(|p| p.project.name.as_str())
        .collect();
    assert_eq!(names, vec!["held", "running", "paused", "done"]);
}

/// Two projects in the same state keep the order the backend sent, so nothing shuffles under
/// the pointer.
#[test]
fn projects_in_the_same_state_keep_the_order_they_arrived_in() {
    let all = vec![
        project("zeta", Status::Active, &[]),
        project("alpha", Status::Active, &[]),
    ];
    let names: Vec<&str> = ordered(&all)
        .iter()
        .map(|p| p.project.name.as_str())
        .collect();
    assert_eq!(names, vec!["zeta", "alpha"]);
}

/// The real fixture has a held up project, and it has to come first or the overview buries
/// the only project anybody has to do something about.
#[test]
fn the_mock_board_puts_the_held_up_project_first() {
    let s = MockPanelDataSource::new().snapshot();
    let first = ordered(&s.projects)[0];
    assert!(
        !first.project.blockers.is_empty(),
        "expected the blocked project at the top, got {}",
        first.project.name
    );
}
