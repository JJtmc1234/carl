//! Giving an agent a folder, and telling the organisation it happened.
//!
//! Two acts, and the difference is who may perform them.
//!
//! **Founding is JJ's.** There is no chief with a folder yet, so there is nobody who could
//! authorise it. Recording the actor as `jj` rather than inventing an authority for it is the
//! honest version.
//!
//! **Enlisting is Carl's.** Who exists and where they sit is protected, and the chief is the
//! only rank that may touch it.
//!
//! One thing to be exact about, because it is easy to read this as more than it is. The
//! organisation in v1 is the fixed table in `army::org`. Enlisting gives a member of that
//! table a folder and announces it. It does not invent a new agent: that means adding a row to
//! `org.rs`, which is a change to a compiled in table and deliberately not something any agent
//! can do at runtime. This is the mechanism the organisation is notified through, and it is
//! the hook onboarding will attach to. It is not a way to grow the army from inside it.

use crate::army::event::{Event, Journal, Record as Logged};
use crate::army::org::{self, Agent};
use crate::army::Rank;
use crate::{Error, Result};

use super::config::Config;
use super::founding::founding_profile;
use super::profile::Profile;
use super::state::State;
use super::store::{Folder, Personnel};

/// Whether this agent may give somebody a folder.
///
/// The chief, and nobody else. JJ's rules for the decisions big enough to need him sit above
/// this and are not built yet.
pub fn may_enlist(actor: &str) -> Result<()> {
    let actor = org::require(actor)?;
    if actor.rank == Rank::Chief {
        return Ok(());
    }
    Err(Error::Refused(format!(
        "{} is a {} and may not enlist anybody. Who exists and where they sit is the chief's, \
         subject to JJ",
        actor.name, actor.rank
    )))
}

/// Writes a folder for every agent in the organisation, and records each one.
///
/// Refuses a home that already holds folders rather than merging into it, because founding run
/// twice by accident should not half rewrite an army that already exists.
pub fn found(home: impl Into<std::path::PathBuf>, now: u64) -> Result<Personnel> {
    let home = home.into();

    // Asked of the disk rather than of the value being built, because the value being built is
    // empty by construction and would answer yes forever.
    if !Personnel::open(&home)?.is_empty() {
        return Err(Error::Refused(
            "this home already holds an army. Founding is for an empty one".into(),
        ));
    }
    let mut army = Personnel::empty(&home)?;

    let mut journal = Journal::open(army.journal_path())?;

    for agent in org::everyone().iter().filter(|a| a.rank != Rank::Human) {
        let profile = founding_profile(agent.name);
        let said = announcement(agent, &profile);
        army.install(Folder {
            agent,
            profile,
            config: Config::default(),
            state: State::fresh(now),
        })?;
        journal.append("jj", Event::Decided { task: None, what: said })?;
    }

    Ok(army)
}

/// Gives one agent a folder, at the request of somebody allowed to ask.
///
/// Everything is checked before anything is written, so a refused enlistment leaves no folder
/// and no event behind.
pub fn enlist(
    army: &mut Personnel,
    journal: &mut Journal,
    actor: &str,
    name: &str,
    profile: Profile,
    config: Config,
    now: u64,
) -> Result<Logged> {
    may_enlist(actor)?;
    let agent = org::require(name)?;
    let said = announcement(agent, &profile);

    army.install(Folder {
        agent,
        profile,
        config,
        state: State::fresh(now),
    })?;

    journal.append(actor, Event::Decided { task: None, what: said })
}

/// What the rest of the army is told about a newcomer.
///
/// The role and where they sit, which is the minimum anybody needs to decide whether this
/// person is anything to do with them. Everything an agent would actually hand over to help a
/// newcomer start is onboarding, and that is a later piece of work.
pub fn announcement(agent: &Agent, profile: &Profile) -> String {
    let answering = match agent.reports_to {
        Some(above) => format!(", answering to {above}"),
        None => String::new(),
    };
    format!(
        "enlisted {} as {} in {}{}. Remit: {}",
        agent.name,
        agent.display,
        profile.place(),
        answering,
        agent.remit
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::army::event;

    #[test]
    fn founding_gives_every_agent_a_folder_and_leaves_jj_without_one() {
        let d = tempfile::tempdir().unwrap();
        let army = found(d.path(), 100).unwrap();

        assert_eq!(army.len(), 4, "carl, adrian, mason and nora");
        assert!(army.get("jj").is_none(), "JJ is not an agent");
        assert!(!d.path().join("army").join("jj").exists());
        assert!(army.missing().is_empty(), "nobody in the table was left out");
    }

    #[test]
    fn founding_records_every_enlistment_in_the_journal() {
        let d = tempfile::tempdir().unwrap();
        let army = found(d.path(), 100).unwrap();

        let all = event::read(army.journal_path()).unwrap();
        assert_eq!(all.len(), 4);
        assert!(all.iter().all(|r| r.actor == "jj"), "founding is JJ's act");
        assert!(
            all.iter().any(|r| matches!(&r.event, Event::Decided { what, .. } if what.contains("nora"))),
            "and every one of them is named"
        );
    }

    #[test]
    fn founding_an_army_that_already_exists_is_refused() {
        let d = tempfile::tempdir().unwrap();
        found(d.path(), 100).unwrap();
        assert!(found(d.path(), 200).is_err());
    }

    /// Who exists and where they sit is a change to the shape of the organisation, so it is
    /// the chief's alone.
    #[test]
    fn only_the_chief_may_enlist() {
        assert!(may_enlist("carl").is_ok());
        for other in ["adrian", "mason", "nora"] {
            let err = may_enlist(other).unwrap_err().to_string();
            assert!(err.contains("may not enlist"), "{other}: {err}");
        }
    }

    #[test]
    fn a_worker_asking_to_enlist_somebody_is_refused_and_writes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let mut army = found(d.path(), 100).unwrap();
        let mut journal = Journal::open(army.journal_path()).unwrap();

        let err = enlist(
            &mut army,
            &mut journal,
            "nora",
            "mason",
            Profile::default(),
            Config::default(),
            200,
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("may not enlist"), "{err}");
        assert_eq!(event::read(army.journal_path()).unwrap().len(), 4, "nothing announced");
    }

    #[test]
    fn enlisting_somebody_who_already_has_a_folder_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let mut army = found(d.path(), 100).unwrap();
        let mut journal = Journal::open(army.journal_path()).unwrap();

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
        assert!(err.contains("already has a folder"), "{err}");
    }

    #[test]
    fn enlisting_somebody_who_is_not_in_the_organisation_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let mut army = found(d.path(), 100).unwrap();
        let mut journal = Journal::open(army.journal_path()).unwrap();

        let err = enlist(
            &mut army,
            &mut journal,
            "carl",
            "piper",
            Profile::default(),
            Config::default(),
            200,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("no agent called piper"), "{err}");
        assert!(err.contains("organisation is"), "and it says who does exist: {err}");
    }

    /// The enlistment path in full, on an agent whose folder was removed.
    #[test]
    fn carl_may_enlist_and_the_organisation_is_told_where_the_newcomer_sits() {
        let d = tempfile::tempdir().unwrap();
        found(d.path(), 100).unwrap();
        std::fs::remove_dir_all(d.path().join("army").join("nora")).unwrap();

        let mut army = Personnel::open(d.path()).unwrap();
        assert_eq!(army.missing().len(), 1);

        let mut journal = Journal::open(army.journal_path()).unwrap();
        let logged = enlist(
            &mut army,
            &mut journal,
            "carl",
            "nora",
            founding_profile("nora"),
            Config::default(),
            200,
        )
        .unwrap();

        assert_eq!(logged.actor, "carl");
        assert_eq!(logged.seq, 5, "the journal carried on from founding");
        let Event::Decided { what, .. } = &logged.event else {
            panic!("wrong event");
        };
        assert!(what.contains("nora"));
        assert!(what.contains("factorio sub department"));
        assert!(what.contains("answering to mason"), "{what}");
        assert!(army.missing().is_empty());
    }

    #[test]
    fn an_announcement_names_the_role_and_the_place() {
        let carl = org::require("carl").unwrap();
        let said = announcement(carl, &founding_profile("carl"));
        assert!(said.contains("carl"));
        assert!(said.contains("answering to jj"), "{said}");
    }
}
