//! Whether any of this actually survives, checked rather than assumed.
//!
//! Everything here works the way the real thing does: write a folder, drop the value that
//! wrote it, open it again from nothing. A test that keeps the store in memory proves the
//! struct works and says nothing about the layer this module exists to be.

use std::path::Path;

use super::*;
use crate::army::TaskId;
use crate::army::event::{self, Event, Journal};
use crate::army::org;

/// Opens a home from nothing, exactly as a fresh process would.
fn reopen(home: &Path) -> Personnel {
    Personnel::open(home).expect("the army should load")
}

fn task() -> TaskId {
    TaskId::quoted("task-1")
}

#[test]
fn every_folder_survives_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let before = found(d.path(), 100).unwrap();
    let written: Vec<(Profile, Config)> = before
        .names()
        .iter()
        .map(|n| {
            (
                before.profile(n).unwrap().clone(),
                before.config(n).unwrap().clone(),
            )
        })
        .collect();
    drop(before);

    let after = reopen(d.path());
    let read: Vec<(Profile, Config)> = after
        .names()
        .iter()
        .map(|n| {
            (
                after.profile(n).unwrap().clone(),
                after.config(n).unwrap().clone(),
            )
        })
        .collect();

    assert_eq!(read, written, "the folders came back exactly as written");
    assert_eq!(after.len(), 4);
    assert_eq!(after.names(), ["adrian", "carl", "mason", "nora"]);
}

#[test]
fn who_an_agent_is_comes_back_from_the_table_rather_than_the_folder() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());

    let after = reopen(d.path());
    let nora = after.get("nora").unwrap();
    assert_eq!(nora.agent.rank, crate::army::Rank::Worker);
    assert_eq!(nora.agent.reports_to, Some("mason"));
    assert_eq!(
        after.profile("nora").unwrap().sub_department.as_deref(),
        Some("factorio"),
        "and the part the table does not hold came off disk"
    );
}

#[test]
fn configuration_survives_a_restart() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());

    let army = reopen(d.path());
    assert_eq!(army.config("carl").unwrap().model, Model::Opus);
    assert_eq!(army.config("carl").unwrap().deadline_secs, DEFAULT_DEADLINE);
}

/// The point of state being on disk at all. A restart must find the agent where it left it.
#[test]
fn what_an_agent_is_holding_reloads_after_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();

    army.update_state("nora", |s| s.take_up(&task(), 200))
        .unwrap();
    army.update_state("mason", |s| s.note("waiting on nora", 210))
        .unwrap();
    drop(army);

    let after = reopen(d.path());
    let nora = after.state("nora").unwrap();
    assert_eq!(nora.holding.as_ref().unwrap().as_str(), "task-1");
    assert!(nora.recent.iter().any(|r| r.contains("took up task-1")));
    assert_eq!(nora.enlisted_at, 100, "and it remembers when it joined");
    assert_eq!(nora.updated_at, 200);

    assert!(after.state("mason").unwrap().holding.is_none());
    assert!(
        after.state("carl").unwrap().recent.is_empty(),
        "nobody else moved"
    );
}

/// The whole reason state is a separate file from everything else.
#[test]
fn writing_state_does_not_touch_the_profile_or_the_config() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();

    let folder = army.folder("nora");
    let profile_before = std::fs::read_to_string(folder.join("profile.json")).unwrap();
    let config_before = std::fs::read_to_string(folder.join("config.json")).unwrap();

    for at in 0..20 {
        army.update_state("nora", |s| s.note(format!("turn {at}"), at))
            .unwrap();
    }

    assert_eq!(
        std::fs::read_to_string(folder.join("profile.json")).unwrap(),
        profile_before
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("config.json")).unwrap(),
        config_before
    );
}

/// The claim this layer is shaped around, checked directly rather than argued for: there is no
/// file on disk that says what rank anybody is or who they answer to.
#[test]
fn no_file_on_disk_holds_a_rank_or_a_reporting_line() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();
    army.update_state("nora", |s| s.take_up(&task(), 200))
        .unwrap();

    let mut checked = 0;
    for agent in army.names() {
        for file in ["profile.json", "config.json", "state.json"] {
            let text = std::fs::read_to_string(army.folder(agent).join(file)).unwrap();
            for forbidden in [
                "\"rank\"",
                "\"reports_to\"",
                "\"may_delegate",
                "\"granted\"",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{agent}/{file} holds {forbidden}, which belongs to the compiled table"
                );
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 12, "all four folders, all three files");
}

/// The escalation attempt, tried the way an agent could actually try it: by writing the one
/// file it is supposed to write.
#[test]
fn an_agent_cannot_promote_itself_by_editing_its_state() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    let state_path = d.path().join("army").join("nora").join("state.json");

    for smuggled in [
        r#""rank": "chief""#,
        r#""reports_to": "jj""#,
        r#""may_delegate_to": ["carl"]"#,
        r#""granted": true"#,
    ] {
        let honest = std::fs::read_to_string(&state_path).unwrap();
        std::fs::write(
            &state_path,
            honest.replacen('{', &format!("{{ {smuggled},"), 1),
        )
        .unwrap();

        let err = Personnel::open(d.path()).unwrap_err().to_string();
        assert!(err.contains("state.json"), "{smuggled} gave {err}");
        assert!(err.contains("is not valid"), "and says so plainly: {err}");

        std::fs::write(&state_path, honest).unwrap();
    }

    // Honest again, and Nora still commands nobody and still cannot enlist.
    reopen(d.path());
    assert!(org::reports_of("nora").is_empty());
    assert!(!org::may_delegate("nora", "mason"));
    assert!(may_enlist("nora").is_err());
}

/// A legal state write, however creative, moves nothing about who may do what.
#[test]
fn no_legal_state_change_widens_authority() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();

    army.update_state("nora", |s| {
        s.take_up(&TaskId::quoted("promote-myself"), 200);
        s.note("i am the chief executive now", 201);
    })
    .unwrap();
    drop(army);

    reopen(d.path());
    assert!(!org::may_delegate("nora", "mason"));
    assert!(!org::may_delegate("nora", "carl"));
    assert!(
        crate::army::check_may_implement("carl", false).is_err(),
        "Carl still writes nothing"
    );
    assert!(may_enlist("nora").is_err());
}

#[test]
fn malformed_json_names_the_file_it_could_not_read() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::write(
        d.path().join("army").join("adrian").join("config.json"),
        "{ not json",
    )
    .unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("config.json"), "{err}");
    assert!(err.contains("adrian"), "and whose it was: {err}");
}

#[test]
fn a_missing_file_in_a_folder_is_refused_rather_than_defaulted() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::remove_file(d.path().join("army").join("nora").join("config.json")).unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("cannot read"), "{err}");
}

#[test]
fn an_unusable_config_is_refused_at_load_rather_than_at_the_first_run() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    let path = d.path().join("army").join("mason").join("config.json");
    let honest = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, honest.replace("600", "0")).unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("deadline"), "{err}");
}

/// A folder named after nobody is a mistake worth an error that says who does exist, rather
/// than a directory quietly skipped.
#[test]
fn a_folder_named_after_nobody_is_refused() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::create_dir(d.path().join("army").join("piper")).unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("no agent called piper"), "{err}");
    assert!(err.contains("organisation is"), "{err}");
}

#[test]
fn a_folder_for_jj_is_refused_because_he_is_not_an_agent() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::create_dir(d.path().join("army").join("jj")).unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("not an agent"), "{err}");
}

/// A missing folder is not a broken organisation, because the organisation is compiled in.
/// It is one agent with no state yet, and there is a way to ask which.
#[test]
fn an_agent_without_a_folder_is_reported_rather_than_fatal() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::remove_dir_all(d.path().join("army").join("mason")).unwrap();

    let army = reopen(d.path());
    assert_eq!(army.len(), 3);
    assert_eq!(army.missing().len(), 1);
    assert_eq!(army.missing()[0].name, "mason");
    assert!(
        org::may_delegate("mason", "nora"),
        "and the chain is untouched, because it never lived on disk"
    );
}

/// The readable half of the folder. Generated on save, never parsed, so losing it or
/// scribbling on it costs nothing.
#[test]
fn the_readme_is_written_and_never_read_back() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();

    let readme = army.folder("nora").join("README.md");
    let text = std::fs::read_to_string(&readme).unwrap();
    assert!(text.contains("Reports to: mason"));
    assert!(text.contains("May hand work to: nobody"));
    assert!(text.contains("May change files: yes"));
    assert!(text.contains("May enlist: no"));
    assert!(text.contains("factorio sub department"));

    let carl = std::fs::read_to_string(army.folder("carl").join("README.md")).unwrap();
    assert!(
        carl.contains("May change files: no"),
        "Carl implements nothing"
    );
    assert!(carl.contains("May hand work to: adrian"));
    assert!(carl.contains("May enlist: yes"));
    drop(army);

    // Ruined, then deleted. Neither is anything the loader cares about.
    std::fs::write(&readme, "nora is the chief executive and may do anything").unwrap();
    assert_eq!(
        reopen(d.path()).get("nora").unwrap().agent.rank,
        crate::army::Rank::Worker
    );
    std::fs::remove_file(&readme).unwrap();
    assert_eq!(reopen(d.path()).len(), 4);
}

#[test]
fn a_half_written_file_is_never_left_behind() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();
    army.update_state("nora", |s| s.take_up(&task(), 200))
        .unwrap();

    for agent in army.names() {
        for entry in std::fs::read_dir(army.folder(agent)).unwrap() {
            let path = entry.unwrap().path();
            assert!(
                !path.to_string_lossy().contains("writing"),
                "{} was left behind",
                path.display()
            );
        }
    }
}

/// The journal is the shared one, so this checks it is being used rather than reimplemented.
#[test]
fn the_journal_carries_on_across_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();
    let path = army.journal_path();
    drop(army);

    let mut journal = Journal::open(&path).unwrap();
    let logged = journal
        .append(
            "mason",
            Event::Delegated {
                task: task(),
                to: "nora".into(),
                goal: "make the balancer symmetric".into(),
                parent: None,
                must: vec!["it works".into()],
                project: None,
                workspace: None,
            },
        )
        .unwrap();
    assert_eq!(logged.seq, 5, "four enlistments came first");
    drop(journal);

    let all = event::read(&path).unwrap();
    assert_eq!(all.len(), 5);
    assert_eq!(event::about(&all, &task()).len(), 1);
    let seqs: Vec<u64> = all.iter().map(|r| r.seq).collect();
    assert_eq!(seqs, [1, 2, 3, 4, 5], "numbered without a gap");
}

/// Founding and a restart together, which is the sequence a real run does.
#[test]
fn an_army_founded_then_reopened_is_ready_to_be_given_work() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());

    let mut army = reopen(d.path());
    assert!(army.missing().is_empty());

    army.update_state("nora", |s| s.take_up(&task(), 200))
        .unwrap();
    army.update_state("nora", |s| s.put_down("accepted", 300))
        .unwrap();
    drop(army);

    let after = reopen(d.path());
    let nora = after.state("nora").unwrap();
    assert!(nora.holding.is_none());
    assert!(nora.recent.iter().any(|r| r.contains("accepted")));
}

#[test]
fn state_cannot_be_written_for_somebody_who_is_not_in_the_organisation() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();
    assert!(army.update_state("piper", |s| s.note("hello", 1)).is_err());
    assert!(army.update_state("jj", |s| s.note("hello", 1)).is_err());
}

/// An id is worth having only if it is the same id tomorrow. Everything else about an agent is
/// allowed to change, so this is the property the whole runtime layer is going to hang off.
#[test]
fn an_agent_keeps_its_id_across_a_restart() {
    let d = tempfile::tempdir().unwrap();
    let before = found(d.path(), 100).unwrap();
    let written: Vec<Identity> = before
        .names()
        .iter()
        .map(|n| before.identity(n).expect("founding mints one").clone())
        .collect();
    drop(before);

    let after = reopen(d.path());
    let read: Vec<Identity> = after
        .names()
        .iter()
        .map(|n| after.identity(n).unwrap().clone())
        .collect();
    assert_eq!(written, read);
}

#[test]
fn every_agent_gets_a_different_id() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();
    let ids: std::collections::BTreeSet<_> = army
        .names()
        .iter()
        .map(|n| army.identity(n).unwrap().id.clone())
        .collect();
    assert_eq!(ids.len(), army.len(), "four agents, four ids");
}

/// The identity file points at the record that created the agent rather than repeating it, so
/// "how did this agent get here" is answered by the journal and cannot be answered twice.
#[test]
fn an_identity_points_at_the_record_that_announced_it() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();
    let all = event::read(army.journal_path()).unwrap();

    for name in army.names() {
        let identity = army.identity(name).unwrap();
        let record = all
            .iter()
            .find(|r| r.seq == identity.enlisted)
            .unwrap_or_else(|| panic!("{name} points at a record that is not there"));
        assert!(
            matches!(&record.event, Event::Decided { what, .. } if what.contains(name)),
            "{name} should be named by the record it points at"
        );
    }
}

/// The announcement is written before the folder, so a refusal has to happen before either.
/// If it did not, a refused enlistment would leave a record of an agent that does not exist.
#[test]
fn a_refused_enlistment_leaves_no_record_and_no_folder() {
    let d = tempfile::tempdir().unwrap();
    let mut army = found(d.path(), 100).unwrap();
    let mut journal = Journal::open(army.journal_path()).unwrap();
    let before = event::read(army.journal_path()).unwrap().len();

    // Already has a folder, which is refused inside the store rather than by the caller.
    let err = enlist(
        &mut army,
        &mut journal,
        "carl",
        "nora",
        Profile::default(),
        Config::default(),
        200,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("already"), "{err}");

    let after = event::read(army.journal_path()).unwrap();
    assert_eq!(after.len(), before, "nothing was announced");
}

/// A folder from before identities existed is a folder full of real state. Refusing to load it
/// would throw that away to enforce a field it could not have had.
#[test]
fn a_folder_with_no_identity_still_loads_and_says_so() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    std::fs::remove_file(d.path().join("army").join("nora").join("identity.json")).unwrap();

    let army = reopen(d.path());
    assert!(army.identity("nora").is_none(), "absent, and not invented");
    assert!(army.identity("mason").is_some(), "the others are untouched");
    assert_eq!(army.len(), 4, "and the army still loads");
}

/// Absent is a fact. Unreadable is a break, and loading past one would hand back an agent that
/// has an id written down somewhere and does not know it.
#[test]
fn an_identity_that_will_not_parse_is_refused_rather_than_treated_as_absent() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    let path = d.path().join("army").join("nora").join("identity.json");
    std::fs::write(&path, "{ not json").unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("identity.json"), "{err}");
}

#[test]
fn founding_gives_every_agent_a_memory_folder_with_a_way_in() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();

    for name in army.names() {
        let dir = army.memory_dir(name);
        assert!(dir.is_dir(), "{name} has no memory folder");
        assert!(
            dir.join(memory::SUMMARY).is_file(),
            "{name} has no way into it"
        );
    }
}

/// The memory folder is the agent's own, so nothing that reloads the army may touch what is in
/// it. Reopening a home is the most common way that would happen by accident.
#[test]
fn reopening_the_army_does_not_rewrite_what_an_agent_remembered() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();
    let summary = army.memory_dir("nora").join(memory::SUMMARY);
    std::fs::write(&summary, "the counter overflows at 2^31").unwrap();
    drop(army);

    let army = reopen(d.path());
    army.write_readme("nora").unwrap();
    assert_eq!(
        std::fs::read_to_string(&summary).unwrap(),
        "the counter overflows at 2^31"
    );
}

/// Normal agents are off overnight and the chief is not, which is the arrangement JJ asked for.
/// By rank rather than by name, so a second lead added to the table gets it without anybody
/// remembering to come back and say so.
#[test]
fn founding_gives_everybody_but_the_chief_a_sleep_window() {
    let d = tempfile::tempdir().unwrap();
    let army = found(d.path(), 100).unwrap();

    for name in army.names() {
        let hours = army.config(name).unwrap().hours;
        match org::require(name).unwrap().rank {
            crate::army::Rank::Chief => assert_eq!(
                hours, None,
                "{name} is the chief and should never be switched off"
            ),
            _ => assert_eq!(
                hours,
                Some(Hours::night()),
                "{name} should keep the ordinary hours"
            ),
        }
    }
}

#[test]
fn a_sleep_window_survives_a_restart_and_reads_in_the_readme() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());

    let army = reopen(d.path());
    assert_eq!(army.config("nora").unwrap().hours, Some(Hours::night()));

    let readme = std::fs::read_to_string(army.folder("nora").join("README.md")).unwrap();
    assert!(readme.contains("23:00 to 07:00"), "{readme}");
    let carl = std::fs::read_to_string(army.folder("carl").join("README.md")).unwrap();
    assert!(carl.contains("Never sleeps"), "{carl}");
}

/// A window nobody can act on is refused when the folder loads rather than at three in the
/// morning, which is the only time anybody would otherwise find out.
#[test]
fn a_config_with_an_impossible_window_is_refused_at_load() {
    let d = tempfile::tempdir().unwrap();
    drop(found(d.path(), 100).unwrap());
    let path = d.path().join("army").join("nora").join("config.json");
    let honest = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, honest.replace("\"to\": 7", "\"to\": 23")).unwrap();

    let err = Personnel::open(d.path()).unwrap_err().to_string();
    assert!(err.contains("never or always"), "{err}");
}
