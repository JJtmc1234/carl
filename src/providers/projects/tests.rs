//! The project store, against real files.

use super::*;

fn id() -> ProjectId {
    ProjectId::new("jjtorio").unwrap()
}

fn store() -> (Projects, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let s = Projects::open(d.path());
    (s, d)
}

fn saved() -> (Projects, tempfile::TempDir) {
    let (s, d) = store();
    let mut p = Project::new(id(), "JJtorio", "A Factorio mod JJ is proud of");
    p.department = Some("coding".into());
    p.phase = "phase 2, belt logic".into();
    p.next_objective = Some("make the balancer symmetric".into());
    s.save(&p).unwrap();
    (s, d)
}

fn milestone(title: &str, at: u64) -> NewMilestone {
    NewMilestone {
        project: id(),
        at,
        title: title.to_string(),
        detail: None,
        evidence: Some("commit abc123".into()),
        achievement: Achievement::FeatureWorks,
        source: Source::Jj,
    }
}

/// Looking at a machine with no projects must not create the store.
#[test]
fn an_empty_home_has_no_projects_and_gains_no_directory() {
    let (s, d) = store();
    assert!(s.list().unwrap().is_empty());
    assert!(s.get(&id()).unwrap().is_none());
    assert!(s.view(&id()).unwrap().is_none());
    assert!(
        !d.path().join("projects").exists(),
        "reading created the store"
    );
}

#[test]
fn a_project_survives_a_restart() {
    let (s, d) = saved();
    drop(s);

    let reopened = Projects::open(d.path());
    let back = reopened.get(&id()).unwrap().unwrap();
    assert_eq!(back.name, "JJtorio");
    assert_eq!(back.department.as_deref(), Some("coding"));
    assert_eq!(back.phase, "phase 2, belt logic");
    assert_eq!(back.status, Status::Active);
    assert_eq!(reopened.list().unwrap().len(), 1);
}

#[test]
fn a_project_that_says_nothing_is_refused_before_anything_is_written() {
    let (s, d) = store();
    let empty = Project::new(id(), "JJtorio", "  ");
    assert!(s.save(&empty).is_err());
    assert!(!d.path().join("projects").join("jjtorio").exists());
}

#[test]
fn milestones_are_appended_and_come_back_in_order() {
    let (s, _d) = saved();
    s.record(milestone("belts balance", 100)).unwrap();
    s.record(milestone("first real test passed", 200)).unwrap();
    s.record(milestone("phase 2 done", 300)).unwrap();

    let all = s.milestones(&id()).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].title, "belts balance", "oldest first");
    assert_eq!(all[0].id, "jjtorio-1");
    assert_eq!(all[2].id, "jjtorio-3", "ids are assigned by the store");

    let recent = s.recent_milestones(&id(), 2).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(
        recent[0].title, "phase 2 done",
        "newest first for the panel"
    );
}

#[test]
fn milestones_survive_a_restart() {
    let (s, d) = saved();
    s.record(milestone("belts balance", 100)).unwrap();
    drop(s);

    let reopened = Projects::open(d.path());
    let back = reopened.milestones(&id()).unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].evidence.as_deref(), Some("commit abc123"));
    assert_eq!(back[0].source, Source::Jj);
    assert_eq!(back[0].achievement, Achievement::FeatureWorks);
}

/// A timeline for a project nobody defined is a row of text nobody can act on.
#[test]
fn a_milestone_for_a_project_that_does_not_exist_is_refused() {
    let (s, _d) = store();
    let err = s
        .record(milestone("out of nowhere", 1))
        .unwrap_err()
        .to_string();
    assert!(err.contains("no project called"), "{err}");
}

#[test]
fn a_milestone_with_no_title_is_refused_and_writes_nothing() {
    let (s, _d) = saved();
    let mut bad = milestone("   ", 1);
    bad.title = "   ".into();
    assert!(s.record(bad).is_err());
    assert!(
        s.milestones(&id()).unwrap().is_empty(),
        "nothing was appended"
    );
}

/// The separation the brief asked for, enforced by the files being different.
#[test]
fn a_suggestion_never_appears_in_the_accepted_history() {
    let (s, d) = saved();
    s.record(milestone("real thing", 100)).unwrap();
    s.suggest(&Suggestion {
        project: id(),
        at: 200,
        title: "the test suite went green".into(),
        detail: None,
        evidence: Some("commit def456".into()),
        because: "a build that was failing now passes".into(),
    })
    .unwrap();

    let accepted = s.milestones(&id()).unwrap();
    assert_eq!(accepted.len(), 1, "the suggestion did not become history");
    assert_eq!(accepted[0].title, "real thing");

    assert_eq!(s.suggestions(&id()).unwrap().len(), 1);
    assert!(d.path().join("projects/jjtorio/suggested.jsonl").exists());
    assert!(d.path().join("projects/jjtorio/milestones.jsonl").exists());
}

#[test]
fn a_suggestion_for_an_unknown_project_is_refused() {
    let (s, _d) = store();
    let err = s
        .suggest(&Suggestion {
            project: id(),
            at: 1,
            title: "x".into(),
            detail: None,
            evidence: None,
            because: "y".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("no project called"), "{err}");
}

#[test]
fn a_view_carries_the_project_and_its_recent_history() {
    let (s, _d) = saved();
    for n in 1..=8 {
        s.record(milestone(&format!("thing {n}"), n as u64 * 100))
            .unwrap();
    }

    let view = s.view(&id()).unwrap().unwrap();
    assert_eq!(view.project.name, "JJtorio");
    assert_eq!(view.milestones.len(), RECENT, "capped for the panel");
    assert_eq!(view.milestones[0].title, "thing 8", "newest first");
    assert!(!view.is_busy(), "no work has been attached");
}

/// The gap, made explicit. Active work is supplied rather than guessed, because nothing in the
/// shared types links a task to a project yet.
#[test]
fn active_work_is_attached_by_the_caller_rather_than_invented() {
    let (s, _d) = saved();
    let view = s.view(&id()).unwrap().unwrap();
    assert!(view.active_tasks.is_empty(), "nothing is assumed");

    let joined = view.with_work(vec![TaskId::quoted("task-1")], vec!["nora".into()]);
    assert!(joined.is_busy());
    assert_eq!(joined.active_agents, ["nora"]);
}

#[test]
fn a_project_file_that_will_not_parse_is_an_error_naming_the_file() {
    let (s, d) = saved();
    std::fs::write(d.path().join("projects/jjtorio/project.json"), "{ not json").unwrap();
    let err = s.get(&id()).unwrap_err().to_string();
    assert!(err.contains("project.json"), "{err}");
}

#[test]
fn a_folder_that_is_not_a_legal_project_id_is_refused() {
    let (s, d) = saved();
    std::fs::create_dir_all(d.path().join("projects").join("Not A Project")).unwrap();
    assert!(s.list().is_err());
}

#[test]
fn writing_leaves_no_staging_file_behind() {
    let (_s, d) = saved();
    let left: Vec<_> = std::fs::read_dir(d.path().join("projects/jjtorio"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("writing"))
        .collect();
    assert!(left.is_empty(), "left behind: {left:?}");
}

#[test]
fn a_project_can_name_a_repository_and_it_is_checked_rather_than_assumed() {
    let d = tempfile::tempdir().unwrap();
    assert!(
        !is_repository(d.path()),
        "an ordinary directory is not a repository"
    );
    std::fs::create_dir(d.path().join(".git")).unwrap();
    assert!(is_repository(d.path()));
}

#[test]
fn a_process_that_has_gone_is_not_still_running() {
    assert!(still_running(std::process::id()), "this one is");
    assert!(!still_running(u32::MAX));
}

// ---- hardening ----

/// A project id becomes a folder name, so traversal has to be impossible at construction
/// rather than guarded at each use.
#[test]
fn no_project_id_can_escape_the_store_directory() {
    for attempt in [
        "../../etc",
        "..",
        "a/../../b",
        "/etc/passwd",
        "a/b",
        "a\\b",
        ".",
        ".hidden",
        "a\0b",
    ] {
        assert!(
            ProjectId::new(attempt).is_err(),
            "{attempt:?} should be refused"
        );
    }

    // And the same check runs on the way in from a file.
    for attempt in ["\"../../etc\"", "\"a/b\"", "\"/etc\""] {
        assert!(
            serde_json::from_str::<ProjectId>(attempt).is_err(),
            "{attempt}"
        );
    }
}

#[test]
fn every_legal_id_stays_directly_under_the_store_root() {
    let (s, d) = store();
    let root = d.path().join("projects");
    for good in ["jjtorio", "a", "aos-2", "x9"] {
        let id = ProjectId::new(good).unwrap();
        let folder = s.folder(&id);
        assert_eq!(folder.parent(), Some(root.as_path()), "{good} escaped");
        assert_eq!(folder.file_name().unwrap(), good);
    }
}

/// The bug a count based id would have caused: one unreadable line and the next milestone
/// reuses an id that is already taken.
#[test]
fn a_corrupt_line_does_not_cause_a_repeated_milestone_id() {
    let (s, d) = saved();
    s.record(milestone("first", 100)).unwrap();
    s.record(milestone("second", 200)).unwrap();

    // Ruin the middle of the file, the way a half written append would.
    let path = d.path().join("projects/jjtorio/milestones.jsonl");
    let good = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = good.lines().collect();
    lines[0] = "{ not json";
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();

    assert_eq!(s.milestones(&id()).unwrap().len(), 1, "one line survived");
    assert_eq!(
        s.milestone_gaps(&id()).unwrap(),
        1,
        "and the hole is visible"
    );

    let third = s.record(milestone("third", 300)).unwrap();
    assert_eq!(third.id, "jjtorio-3", "the id carried on past the hole");

    let ids: Vec<String> = s
        .milestones(&id())
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(ids.len(), unique.len(), "duplicated ids: {ids:?}");
}

#[test]
fn a_readable_history_reports_no_gaps() {
    let (s, _d) = saved();
    s.record(milestone("one", 100)).unwrap();
    assert_eq!(s.milestone_gaps(&id()).unwrap(), 0);
    assert_eq!(s.view(&id()).unwrap().unwrap().milestone_gaps, 0);
}

#[test]
fn a_view_carries_the_number_of_unreadable_lines() {
    let (s, d) = saved();
    s.record(milestone("one", 100)).unwrap();
    let path = d.path().join("projects/jjtorio/milestones.jsonl");
    let good = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("{good}{{ broken\n")).unwrap();

    assert_eq!(s.view(&id()).unwrap().unwrap().milestone_gaps, 1);
}

/// Malformed project JSON must be an error, never an empty project that looks fine.
#[test]
fn a_malformed_project_never_becomes_an_empty_healthy_one() {
    let (s, d) = saved();
    for rubbish in ["{ not json", "", "null", "[]", "{}"] {
        std::fs::write(d.path().join("projects/jjtorio/project.json"), rubbish).unwrap();
        let got = s.get(&id());
        assert!(got.is_err(), "{rubbish:?} loaded as {got:?}");
        assert!(s.list().is_err(), "{rubbish:?} survived a listing");
    }
}

/// A folder with no project.json is not a project, and is not a blank one either.
#[test]
fn a_folder_without_a_project_file_is_absent_rather_than_empty() {
    let (s, d) = store();
    std::fs::create_dir_all(d.path().join("projects").join("halfmade")).unwrap();

    let id = ProjectId::new("halfmade").unwrap();
    assert_eq!(s.get(&id).unwrap(), None);
    assert!(s.list().unwrap().is_empty());
    assert!(s.view(&id).unwrap().is_none());
}

#[test]
fn milestone_order_is_deterministic_even_at_the_same_timestamp() {
    let (s, d) = saved();
    for n in 1..=6 {
        s.record(milestone(&format!("thing {n}"), 1_000)).unwrap();
    }

    let first: Vec<String> = s
        .milestones(&id())
        .unwrap()
        .into_iter()
        .map(|m| m.id)
        .collect();
    drop(s);

    // Read again from a fresh store, repeatedly. Same order every time.
    for _ in 0..5 {
        let again = Projects::open(d.path());
        let ids: Vec<String> = again
            .milestones(&id())
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, first, "append order is the order");
    }
    assert_eq!(first[0], "jjtorio-1");
}

#[test]
fn newest_first_is_deterministic_and_capped() {
    let (s, _d) = saved();
    for n in 1..=6 {
        s.record(milestone(&format!("thing {n}"), 1_000)).unwrap();
    }

    let a = s.recent_milestones(&id(), 3).unwrap();
    let b = s.recent_milestones(&id(), 3).unwrap();
    assert_eq!(a, b, "two reads disagree");
    assert_eq!(a.len(), 3);
    assert_eq!(a[0].id, "jjtorio-6", "newest first");
    assert_eq!(a[2].id, "jjtorio-4");
}

/// Repeated and interleaved reads must not disturb the store.
#[test]
fn repeated_reads_are_safe_and_do_not_change_anything() {
    let (s, d) = saved();
    s.record(milestone("one", 100)).unwrap();

    let before =
        std::fs::read_to_string(d.path().join("projects/jjtorio/milestones.jsonl")).unwrap();
    for _ in 0..20 {
        s.list().unwrap();
        s.get(&id()).unwrap();
        s.milestones(&id()).unwrap();
        s.recent_milestones(&id(), 3).unwrap();
        s.suggestions(&id()).unwrap();
        s.view(&id()).unwrap();
    }
    let after =
        std::fs::read_to_string(d.path().join("projects/jjtorio/milestones.jsonl")).unwrap();
    assert_eq!(before, after, "reading changed the history");
}

/// The two stores must not be able to impersonate each other through their own files.
#[test]
fn a_suggestion_file_cannot_be_read_as_history_and_the_reverse() {
    let (s, d) = saved();
    s.record(milestone("real", 100)).unwrap();
    s.suggest(&Suggestion {
        project: id(),
        at: 200,
        title: "proposed".into(),
        detail: None,
        evidence: None,
        because: "a build went green".into(),
    })
    .unwrap();

    let folder = d.path().join("projects/jjtorio");
    let history = std::fs::read_to_string(folder.join("milestones.jsonl")).unwrap();
    let proposals = std::fs::read_to_string(folder.join("suggested.jsonl")).unwrap();

    for line in proposals.lines() {
        assert!(
            serde_json::from_str::<Milestone>(line).is_err(),
            "a suggestion parsed as a milestone: {line}"
        );
    }
    for line in history.lines() {
        assert!(
            serde_json::from_str::<Suggestion>(line).is_err(),
            "a milestone parsed as a suggestion: {line}"
        );
    }

    // And swapping the files wholesale yields nothing rather than a mixed history.
    std::fs::write(folder.join("milestones.jsonl"), &proposals).unwrap();
    assert!(
        s.milestones(&id()).unwrap().is_empty(),
        "nothing was adopted"
    );
    assert_eq!(
        s.milestone_gaps(&id()).unwrap(),
        1,
        "and the mismatch is visible"
    );
}

/// recent_changes.json is a governance feed, not a milestone source. Nothing here reads it.
#[test]
fn the_governance_feed_is_not_a_milestone_source() {
    let (s, d) = saved();
    std::fs::write(
        d.path().join("recent_changes.json"),
        r#"{"recent_changes":[{"id":"aos-1","title":"JJ is ultimate authority"}]}"#,
    )
    .unwrap();

    assert!(
        s.milestones(&id()).unwrap().is_empty(),
        "a governance file must not become project history"
    );
    assert_eq!(s.list().unwrap().len(), 1, "and it is not a project either");
}
