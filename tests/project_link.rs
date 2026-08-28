//! Which project a task belongs to, and that it survives being written down and read back.
//!
//! A `Task` never touches the disk. Its project identity survives a restart only by riding on
//! the `Delegated` event, so the questions worth asking here are all about the round trip: does
//! it go into the journal, does it come back out of the fold, and does a journal written before
//! any of this existed still open.
//!
//! The old format lines below are real. They were produced by the code as it stood before
//! projects, pasted in rather than generated, because a compatibility test that builds its own
//! idea of the old format only proves the two ideas agree.

use carl::ProjectId;
use carl::army::event::{Event, Journal, Record, read};
use carl::army::task::{Status, Task, Verification};
use carl::panel::tasks;

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

fn jjtorio() -> ProjectId {
    ProjectId::new("jjtorio").unwrap()
}

#[test]
fn a_task_belongs_to_no_project_unless_somebody_says_so() {
    let t = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    assert_eq!(t.project, None);
    assert_eq!(t.workspace, None);
}

#[test]
fn a_task_can_be_given_a_project_when_it_is_created() {
    let t = Task::assign("mason", "nora", "cache the lookup", verification())
        .unwrap()
        .for_project(jjtorio());
    assert_eq!(t.project, Some(jjtorio()));
}

/// Work split off a project's task is that project's work.
///
/// Inherited rather than asked for, so a lead cannot drop a subtask out of its project by
/// forgetting to mention it, which is the failure nobody would ever notice.
#[test]
fn a_split_task_inherits_its_parents_project() {
    let parent = Task::assign("carl", "mason", "the Factorio side", verification())
        .unwrap()
        .for_project(jjtorio());
    let child =
        Task::split_from(&parent, "mason", "nora", "cache the lookup", verification()).unwrap();

    assert_eq!(child.project, Some(jjtorio()));
    assert_eq!(child.parent, Some(parent.id));
}

#[test]
fn a_split_of_an_unprojected_task_stays_unprojected() {
    let parent = Task::assign("carl", "mason", "some errand", verification()).unwrap();
    let child = Task::split_from(&parent, "mason", "nora", "part of it", verification()).unwrap();
    assert_eq!(child.project, None, "nothing was inherited from nothing");
}

/// Two different facts that would be one field if somebody were careless.
///
/// A worktree is where the files are. A project is whose work it is. Two tasks can share a
/// checkout and serve different projects, and one project can span several checkouts.
#[test]
fn a_workspace_and_a_project_are_independent() {
    let mut t = Task::assign("mason", "nora", "cache the lookup", verification())
        .unwrap()
        .for_project(jjtorio());
    t.workspace = Some("/home/jj/worktrees/cache".into());

    assert_eq!(t.project, Some(jjtorio()));
    assert_eq!(t.workspace.as_deref(), Some("/home/jj/worktrees/cache"));

    let shared = Task::assign("mason", "nora", "something else", verification())
        .unwrap()
        .for_project(ProjectId::new("carl").unwrap());
    assert_ne!(
        t.project, shared.project,
        "the same checkout could serve either"
    );
}

// ───────────────────────── the journal round trip ─────────────────────────

fn delegate(journal: &mut Journal, t: &Task) {
    journal
        .append(
            &t.created_by,
            Event::Delegated {
                task: t.id.clone(),
                to: t.owner.clone(),
                goal: t.goal.clone(),
                parent: t.parent.clone(),
                must: t.verification.must.clone(),
                project: t.project.clone(),

                workspace: t.workspace.clone(),
                objective: None,
            },
        )
        .unwrap();
}

#[test]
fn a_projects_identity_survives_being_written_down_and_folded_back() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("events.jsonl");
    let mut journal = Journal::open(&at).unwrap();

    let t = Task::assign("mason", "nora", "cache the lookup", verification())
        .unwrap()
        .for_project(jjtorio());
    delegate(&mut journal, &t);

    let rebuilt = &tasks::fold(&read(&at).unwrap())[0];
    assert_eq!(rebuilt.project, Some(jjtorio()));
    assert_eq!(rebuilt.id, t.id.to_string());
    assert_eq!(rebuilt.owner, "nora");
}

#[test]
fn a_task_with_no_project_folds_back_with_no_project() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("events.jsonl");
    let mut journal = Journal::open(&at).unwrap();

    let t = Task::assign("mason", "nora", "an errand", verification()).unwrap();
    delegate(&mut journal, &t);

    assert_eq!(tasks::fold(&read(&at).unwrap())[0].project, None);
}

/// A line produced before projects existed. Not generated: pasted.
///
/// Generating the old format from today's code would only prove today's code agrees with
/// itself. This is what was actually being written, and an old journal is not a broken journal:
/// refusing to open one would throw away the only record of everything that happened before.
const BEFORE_PROJECTS: &str = concat!(
    r#"{"seq":1,"at":1755200000,"actor":"mason","event":"delegated","#,
    r#""task":"a1b2c3","to":"nora","goal":"cache the prototype lookup"}"#,
);

/// Older still, from before the parent and verification conditions were carried either.
const BEFORE_ANYTHING: &str = concat!(
    r#"{"seq":2,"at":1755200001,"actor":"nora","event":"moved","#,
    r#""task":"a1b2c3","from":"assigned","to":"in hand"}"#,
);

#[test]
fn a_journal_written_before_projects_existed_still_opens() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("events.jsonl");
    std::fs::write(&at, format!("{BEFORE_PROJECTS}\n{BEFORE_ANYTHING}\n")).unwrap();

    let records = read(&at).unwrap();
    assert_eq!(records.len(), 2, "both lines read: {records:?}");

    match &records[0].event {
        Event::Delegated {
            project,
            parent,
            must,
            goal,
            ..
        } => {
            assert_eq!(*project, None, "nobody said, which is not the same as none");
            assert_eq!(*parent, None);
            assert!(must.is_empty());
            assert_eq!(goal, "cache the prototype lookup");
        }
        other => panic!("wrong event: {other:?}"),
    }

    let rebuilt = &tasks::fold(&records)[0];
    assert_eq!(rebuilt.project, None);
    assert_eq!(rebuilt.status, "in hand", "and the rest still folds");
}

/// And a new line written next to old ones reads back correctly, so a journal mid upgrade works.
#[test]
fn old_and_new_lines_live_in_one_journal() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("events.jsonl");
    std::fs::write(&at, format!("{BEFORE_PROJECTS}\n")).unwrap();

    let mut journal = Journal::open(&at).unwrap();
    let t = Task::assign("mason", "nora", "the new one", verification())
        .unwrap()
        .for_project(jjtorio());
    delegate(&mut journal, &t);

    let folded = tasks::fold(&read(&at).unwrap());
    assert_eq!(folded.len(), 2);
    assert_eq!(
        folded[0].project, None,
        "the old one still says nobody said"
    );
    assert_eq!(folded[1].project, Some(jjtorio()), "and the new one says");
}

/// Nothing outside creation can move a task between projects.
///
/// Checked by asking the type system rather than by calling something: `for_project` consumes
/// `self`, so it cannot be reached through a `&mut` to a task somebody is already holding, and
/// there is no setter. This test is here so that deleting that property fails something.
#[test]
fn a_worker_cannot_move_her_own_task_into_another_project() {
    let mut t = Task::assign("mason", "nora", "cache the lookup", verification())
        .unwrap()
        .for_project(jjtorio());
    t.advance("nora", Status::InHand).unwrap();

    // The only way to a different project is a new task, which goes through `check_delegation`
    // and is therefore Mason's to make and not Nora's.
    assert!(
        Task::assign("nora", "nora", "mine now", verification()).is_err(),
        "a worker cannot even create work for herself"
    );
    assert_eq!(
        t.project,
        Some(jjtorio()),
        "and the one she holds is unmoved"
    );
}

/// The record has to say who put the task in the project, not just that it is in one.
#[test]
fn the_record_says_who_delegated_the_projected_work() {
    let dir = tempfile::tempdir().unwrap();
    let at = dir.path().join("events.jsonl");
    let mut journal = Journal::open(&at).unwrap();

    let t = Task::assign("mason", "nora", "cache the lookup", verification())
        .unwrap()
        .for_project(jjtorio());
    delegate(&mut journal, &t);

    let records: Vec<Record> = read(&at).unwrap();
    assert_eq!(records[0].actor, "mason");
    assert_eq!(tasks::fold(&records)[0].assigner, "mason");
}
