//! The provider backed half of a snapshot, gathered before it is built.
//!
//! Kept apart from `snapshot.rs` because the two halves are gathered on completely different
//! terms. The army half is a few file reads and can be redone whenever anything happens. The
//! machine half costs real work and must not be redone because somebody moved a window.
//!
//! **The rate limit is Process 3's, not a second one here.** `Diagnostics::machine` resamples
//! only when enough time has passed and otherwise hands back the previous readings with their
//! original `measured_at` untouched, so a caller polling quickly sees an honestly old number
//! rather than a fresh looking one. Wrapping that in another cache would be two things deciding
//! how old a number is, which is how a panel ends up showing a timestamp that does not match the
//! value beside it. So this holds one `Diagnostics` for the life of the process and asks it.
//!
//! **Projects join to tasks through the record and nothing else.** A project's active tasks are
//! the tasks whose `project` field names it. Nothing is inferred from a name that looks similar
//! or a path that happens to match, because a guess here would put an agent on a project they
//! were never assigned to, and the panel is what somebody would use to check exactly that.

use super::tasks;
use super::view::TaskView;
use crate::ProjectId;
use crate::providers::projects::{ProjectView, Projects};
use crate::providers::{Diagnostics, Snapshot as Diagnosed};

/// What the providers had to say.
pub struct Facts {
    pub diagnostics: Diagnosed,
    pub projects: Vec<ProjectView>,
}

impl Facts {
    /// Nothing from any provider.
    ///
    /// For callers that only want the army, and for tests that would otherwise sample a real
    /// machine and get a different answer on every run.
    pub fn army_only() -> Self {
        Self {
            diagnostics: Diagnosed {
                army: Vec::new(),
                machine: Vec::new(),
            },
            projects: Vec::new(),
        }
    }

    /// Asks both providers, and joins projects to the tasks that name them.
    ///
    /// `tasks` is the fold of the journal, done once by the caller and passed in, because the
    /// snapshot needs it too and folding twice would be the same work done to get the same
    /// answer.
    pub fn gather(diagnostics: &mut Diagnostics, projects: &Projects, tasks: &[TaskView]) -> Self {
        Self::gather_at(diagnostics, projects, tasks, crate::army::event::now())
    }

    /// The same, as of a given moment.
    ///
    /// The clock is a parameter so the rate limit can be tested without waiting for real seconds
    /// to pass, which is the convention the provider layer already uses.
    pub fn gather_at(
        diagnostics: &mut Diagnostics,
        projects: &Projects,
        tasks: &[TaskView],
        at: u64,
    ) -> Self {
        let listed = projects.list().unwrap_or_default();
        let mut views = Vec::new();

        for project in listed {
            // `view` reads the real milestones, which are only ever written explicitly. Nothing
            // derives one from a commit: a project with a busy repository and no recorded
            // milestones has no milestones, and saying otherwise would be inventing history.
            let Ok(Some(view)) = projects.view(&project.id) else {
                continue;
            };
            let (linked, agents) = working_on(tasks, &project.id);
            views.push(view.with_work(linked, agents));
        }

        Self {
            // `snapshot_at` decides for itself whether anything expensive is due. Asking it
            // often is free, and there is no second cache here that could disagree with it.
            diagnostics: diagnostics.snapshot_at(at),
            projects: views,
        }
    }
}

/// The unfinished tasks that name this project, and who is holding them.
///
/// Empty when nothing names it. A project with no linked tasks shows no tasks, rather than
/// showing everything that might plausibly belong to it.
fn working_on(
    tasks: &[TaskView],
    project: &ProjectId,
) -> (Vec<crate::army::task::TaskId>, Vec<String>) {
    let mut linked = Vec::new();
    let mut agents: Vec<String> = Vec::new();

    for task in tasks {
        if task.project.as_ref() != Some(project) || tasks::settled(&task.status) {
            continue;
        }
        linked.push(crate::army::task::TaskId::quoted(task.id.clone()));
        if !agents.contains(&task.owner) {
            agents.push(task.owner.clone());
        }
    }
    (linked, agents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::projects::model::Project;

    fn task(id: &str, owner: &str, project: Option<&str>, status: &str) -> TaskView {
        TaskView {
            id: id.into(),
            goal: "g".into(),
            owner: owner.into(),
            assigner: "mason".into(),
            parent: None,
            project: project.map(|p| ProjectId::new(p).unwrap()),
            status: status.into(),
            attempts: 0,
            must: vec!["it works".into()],
            review: super::super::view::Maybe::Unknown,
            delegated_at: 1,
            updated_at: 1,
        }
    }

    fn jjtorio() -> ProjectId {
        ProjectId::new("jjtorio").unwrap()
    }

    #[test]
    fn only_tasks_that_name_the_project_join_it() {
        let tasks = [
            task("a", "nora", Some("jjtorio"), "in hand"),
            task("b", "nora", None, "in hand"),
            task("c", "mason", Some("something-else"), "in hand"),
        ];
        let (linked, agents) = working_on(&tasks, &jjtorio());

        assert_eq!(linked.len(), 1, "and not the unlinked one: {linked:?}");
        assert_eq!(linked[0].as_str(), "a");
        assert_eq!(agents, vec!["nora"]);
    }

    /// A task nobody put in a project must not drift into one because it is the only project.
    #[test]
    fn an_unlinked_task_never_joins_a_project_by_being_nearby() {
        let tasks = [task("b", "nora", None, "in hand")];
        let (linked, agents) = working_on(&tasks, &jjtorio());
        assert!(linked.is_empty(), "{linked:?}");
        assert!(agents.is_empty(), "and nobody is shown working on it");
    }

    /// Finished work is history. An agent who finished last week is not busy now.
    #[test]
    fn a_settled_task_stops_counting_as_active() {
        let tasks = [
            task("a", "nora", Some("jjtorio"), "accepted"),
            task("b", "adrian", Some("jjtorio"), "abandoned"),
        ];
        let (linked, agents) = working_on(&tasks, &jjtorio());
        assert!(linked.is_empty());
        assert!(agents.is_empty());
    }

    #[test]
    fn one_agent_holding_two_tasks_is_listed_once() {
        let tasks = [
            task("a", "nora", Some("jjtorio"), "in hand"),
            task("b", "nora", Some("jjtorio"), "assigned"),
        ];
        let (linked, agents) = working_on(&tasks, &jjtorio());
        assert_eq!(linked.len(), 2);
        assert_eq!(agents, vec!["nora"]);
    }

    /// A project with no recorded milestones has none. Nothing is derived from a repository.
    #[test]
    fn a_project_with_a_busy_repository_still_has_no_invented_milestones() {
        let dir = tempfile::tempdir().unwrap();
        let projects = Projects::open(dir.path());
        let project = Project::new(jjtorio(), "JJtorio", "make the mod faster");
        projects.save(&project).unwrap();

        let mut diagnostics = Diagnostics::new(dir.path());
        let facts = Facts::gather(&mut diagnostics, &projects, &[]);

        assert_eq!(facts.projects.len(), 1);
        assert!(
            facts.projects[0].milestones.is_empty(),
            "nothing wrote one, so there are none"
        );
        assert!(facts.projects[0].active_tasks.is_empty());
    }
}
