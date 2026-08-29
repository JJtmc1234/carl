use super::*;
use crate::army::personnel::{found, memory};

fn founded() -> tempfile::TempDir {
    let d = tempfile::tempdir().expect("a temp home");
    found(d.path(), 0).expect("found an army");
    d
}

/// An unfounded home is empty, not an error. Asking about an army that does not exist yet is a
/// reasonable thing to do and the honest answer is that nobody is there.
#[test]
fn an_unfounded_home_reports_everybody_as_not_enlisted() {
    let d = tempfile::tempdir().expect("a temp home");
    let all = everyone(d.path()).expect("a survey");

    assert_eq!(all.len(), org::everyone().len() - 1, "JJ is not an agent");
    assert!(all.iter().all(|s| !s.enlisted));
    assert!(all.iter().all(|s| s.worry().is_some()));
    assert!(
        all[0].worry().expect("a worry").contains("no folder"),
        "{:?}",
        all[0].worry()
    );
}

/// Chart order, because a list sorted by name puts Miles between Mason and Nora where nobody is
/// looking for him.
#[test]
fn the_survey_comes_back_in_chart_order_and_never_includes_jj() {
    let d = founded();
    let names: Vec<&str> = everyone(d.path())
        .expect("a survey")
        .iter()
        .map(|s| s.agent.name)
        .collect();

    assert!(!names.contains(&"jj"), "JJ is the authority, not an agent");
    let carl = names.iter().position(|n| *n == "carl").expect("carl");
    let olivia = names.iter().position(|n| *n == "olivia").expect("olivia");
    let miles = names.iter().position(|n| *n == "miles").expect("miles");
    assert!(carl < olivia && olivia < miles, "{names:?}");
}

/// The reporting line comes from the compiled table, so it is the same one delegation enforces.
#[test]
fn the_reports_match_the_organisation() {
    let d = founded();
    let all = everyone(d.path()).expect("a survey");
    let of = |name: &str| {
        all.iter()
            .find(|s| s.agent.name == name)
            .expect("an agent")
            .reports
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>()
    };
    assert_eq!(of("olivia"), ["miles"]);
    assert_eq!(of("miles"), Vec::<&str>::new());
    assert!(of("carl").contains(&"olivia"));
    assert!(!of("carl").contains(&"miles"), "carl must not reach miles");
}

/// A freshly founded army has nothing wrong with it, or founding is not doing its job.
#[test]
fn a_founded_army_has_nothing_to_worry_about() {
    let d = founded();
    for s in everyone(d.path()).expect("a survey") {
        assert!(s.enlisted, "{} was not enlisted", s.agent.name);
        assert_eq!(s.worry(), None, "{}: {:?}", s.agent.name, s.worry());
    }
}

/// The failure the whole thing exists for. Nobody has said anything about the process, and the
/// agent still cannot work, and that has to be visible.
#[test]
fn a_broken_memory_folder_is_a_worry_even_with_no_runtime_record() {
    let d = founded();
    let people = Personnel::open(d.path()).expect("personnel");
    std::fs::remove_dir_all(memory::dir(&people.folder("miles"))).expect("remove");

    let miles = one(d.path(), "miles").expect("miles");
    assert!(miles.runtime.is_none(), "no supervisor has run here");
    let worry = miles.worry().expect("a broken folder is a worry");
    assert!(worry.contains("carl army migrate"), "{worry}");
}

/// Nobody having said is not the same as stopped, and the survey must not collapse them.
#[test]
fn no_supervisor_record_is_absent_rather_than_stopped() {
    let d = founded();
    assert!(
        everyone(d.path())
            .expect("a survey")
            .iter()
            .all(|s| s.runtime.is_none())
    );
}

#[test]
fn asking_about_somebody_who_is_not_an_agent_is_refused_by_name() {
    let d = founded();
    let why = match one(d.path(), "hunter") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("hunter is not an agent and must not have a standing"),
    };
    assert!(why.contains("hunter"), "{why}");
}

// Activity.

/// A home with no journal has no activity, and that is not an error either.
#[test]
fn no_journal_means_no_activity_rather_than_a_failure() {
    let d = tempfile::tempdir().expect("a temp home");
    assert!(activity(d.path(), None, 20).expect("activity").is_empty());
}

/// Founding writes the enlistment of every agent, so there is something real to fold.
#[test]
fn activity_is_bounded_and_newest_last() {
    let d = founded();
    let all = activity(d.path(), None, 1000).expect("activity");
    assert!(!all.is_empty(), "founding recorded nothing");

    let seqs: Vec<u64> = all.iter().map(|r| r.seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted, "activity is not in order");

    let three = activity(d.path(), None, 3).expect("activity");
    assert_eq!(three.len(), 3, "the bound was not applied");
    assert_eq!(
        three.last().map(|r| r.seq),
        all.last().map(|r| r.seq),
        "the bound kept the oldest instead of the newest"
    );
}

/// An agent's activity is what it did and what was done to it. Filtering on the actor alone
/// would hide the handoff that explains why it is holding what it is holding.
#[test]
fn an_agents_activity_includes_things_done_to_it() {
    let d = founded();
    let mine = activity(d.path(), Some("miles"), 100).expect("activity");
    assert!(
        !mine.is_empty(),
        "miles was enlisted by jj and that is his business"
    );
    assert!(
        mine.iter().any(|r| r.actor != "miles"),
        "only miles's own acts came back, so nothing done to him is visible"
    );
}

#[test]
fn activity_for_somebody_who_is_not_an_agent_is_refused() {
    let d = founded();
    assert!(activity(d.path(), Some("hunter"), 20).is_err());
}

/// The boundary rule, because a substring match would file one agent's history under another.
#[test]
fn a_name_inside_a_longer_word_is_not_a_mention() {
    assert!(names_agent("olivia handed it to miles", "miles"));
    assert!(names_agent(r#"{"to":"miles"}"#, "miles"));
    assert!(names_agent("enlisted miles as Miles", "Miles"), "case");
    assert!(!names_agent("milestone reached", "miles"), "substring");
    assert!(!names_agent("the normal case", "nora"), "substring");
}
