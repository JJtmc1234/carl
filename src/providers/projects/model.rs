//! What a project is, and what counts as having got somewhere.
//!
//! Carl has never had a notion of a project before this, so these are the first definitions
//! rather than a second copy of somebody else's. They are kept deliberately thin.
//!
//! **The milestone is the interesting type.** The brief for this work was that a normal commit
//! is not a milestone, and the way that is enforced here is structural rather than by asking
//! nicely: a milestone has to declare which *kind* of achievement it is, from a closed list,
//! and it has to say who says so. There is no `Other` and no free text kind. Something that
//! does not fit one of the six is not a milestone, and a stream of commits cannot become one by
//! accident because no commit carries a `Source`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// A project's name on disk, which is also its folder.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new(raw: impl Into<String>) -> Result<Self> {
        let raw = raw.into();
        let ok = !raw.is_empty()
            && raw.len() <= 64
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !raw.starts_with('-')
            && !raw.ends_with('-');
        if ok {
            Ok(Self(raw))
        } else {
            Err(Error::Refused(format!(
                "a project id must be lowercase letters, digits and dashes, got {raw:?}"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProjectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ProjectId {
    type Error = Error;
    fn try_from(value: String) -> Result<Self> {
        Self::new(value)
    }
}

impl From<ProjectId> for String {
    fn from(value: ProjectId) -> Self {
        value.0
    }
}

/// Where a project is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    Active,
    /// Real, and nobody is working on it right now.
    Paused,
    Done,
    /// Stopped on purpose and not coming back.
    Abandoned,
}

/// What kind of achievement a milestone records.
///
/// A closed list on purpose. The question "which of these is it" is the thing that stops a
/// commit becoming a milestone, because a commit is usually none of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Achievement {
    /// Something a person wanted now works, for the first time.
    FeatureWorks,
    /// A phase of the plan is finished.
    PhaseCompleted,
    /// A whole class of bug is now prevented rather than fixed once.
    BugClassEliminated,
    /// Two real things were joined and worked together.
    IntegrationSucceeded,
    Released,
    /// The shape of the thing changed, on purpose.
    ArchitectureChanged,
}

/// Who says this happened.
///
/// A milestone with no source is not accepted, because the entire point of the type is that
/// somebody stood behind it. `Journal` is the only automatic source and it names the event it
/// came from, so it can be checked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    Jj,
    Carl,
    /// A department or sub department lead, by agent name.
    Lead(String),
    /// Derived from a verified entry in the army journal, quoting its sequence number.
    Journal {
        seq: u64,
    },
}

/// One meaningful thing that happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Milestone {
    pub id: String,
    pub project: ProjectId,
    /// Unix seconds.
    pub at: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A commit, a test name, a file, a journal sequence. Something somebody can go and check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    pub achievement: Achievement,
    pub source: Source,
}

impl Milestone {
    /// Refuses a milestone that says nothing.
    ///
    /// A title is required because the panel shows a list of titles, and an empty one is a row
    /// that tells the reader nothing while taking up the space of something that would have.
    pub fn check(&self) -> Result<()> {
        if self.title.trim().is_empty() {
            return Err(Error::Refused("a milestone needs a title".into()));
        }
        if self.id.trim().is_empty() {
            return Err(Error::Refused("a milestone needs an id".into()));
        }
        Ok(())
    }
}

/// A milestone somebody or something proposed, which nobody has accepted yet.
///
/// Kept in a different file from the accepted ones and never merged into them by this layer.
/// The brief was explicit that automatic suggestions must not become history on their own, and
/// the cheapest way to guarantee that is for the accepted store to have no code path that
/// reads this type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suggestion {
    pub project: ProjectId,
    pub at: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<String>,
    /// Why something thought this was worth proposing.
    pub because: String,
}

/// One project, as it is written down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    /// The broad goal, in a sentence.
    pub goal: String,
    pub status: Status,
    /// Where it has got to, in the project's own words. Free text because every project counts
    /// phases differently and a shared enum would fit none of them.
    pub phase: String,
    /// The department that owns it, when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    /// The repository or directory, when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_objective: Option<String>,
    /// What is in the way, in plain words. Written by whoever knows, not derived.
    #[serde(default)]
    pub blockers: Vec<String>,
}

impl Project {
    pub fn new(id: ProjectId, name: &str, goal: &str) -> Self {
        Self {
            id,
            name: name.to_string(),
            goal: goal.to_string(),
            status: Status::Active,
            phase: "not started".to_string(),
            department: None,
            path: None,
            next_objective: None,
            blockers: Vec::new(),
        }
    }

    pub fn check(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(Error::Refused(format!("{} needs a name", self.id)));
        }
        if self.goal.trim().is_empty() {
            return Err(Error::Refused(format!("{} needs a goal", self.id)));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> ProjectId {
        ProjectId::new("jjtorio").unwrap()
    }

    #[test]
    fn ordinary_project_ids_are_accepted() {
        for good in ["jjtorio", "carl", "aos-2", "a"] {
            assert!(ProjectId::new(good).is_ok(), "{good}");
        }
    }

    /// A project id becomes a folder name, so the same path shaped forms are refused as
    /// everywhere else in this codebase.
    #[test]
    fn path_shaped_project_ids_are_refused() {
        for bad in ["", "..", "../etc", "a/b", "A", "a b", ".hidden", "-a", "a-"] {
            assert!(ProjectId::new(bad).is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<ProjectId>("\"../../etc\"").is_err());
    }

    #[test]
    fn a_project_round_trips() {
        let mut p = Project::new(id(), "JJtorio", "A Factorio mod that is good");
        p.department = Some("coding".into());
        p.phase = "phase 2".into();
        let text = serde_json::to_string_pretty(&p).unwrap();
        assert_eq!(serde_json::from_str::<Project>(&text).unwrap(), p);
        assert!(p.check().is_ok());
    }

    #[test]
    fn a_project_with_no_goal_is_refused() {
        let mut p = Project::new(id(), "JJtorio", "");
        assert!(p.check().is_err());
        p.goal = "something".into();
        p.name = "  ".into();
        assert!(p.check().is_err());
    }

    fn milestone() -> Milestone {
        Milestone {
            id: "m-1".into(),
            project: id(),
            at: 1_000,
            title: "the belt balancer is symmetric".into(),
            detail: Some("first time it balanced under load".into()),
            evidence: Some("commit abc123".into()),
            achievement: Achievement::FeatureWorks,
            source: Source::Jj,
        }
    }

    #[test]
    fn a_milestone_round_trips_with_its_kind_and_source() {
        let m = milestone();
        let text = serde_json::to_string(&m).unwrap();
        assert!(text.contains("feature-works"));
        assert!(text.contains("\"source\":\"jj\""), "{text}");
        assert_eq!(serde_json::from_str::<Milestone>(&text).unwrap(), m);
    }

    #[test]
    fn a_journal_sourced_milestone_names_the_entry_it_came_from() {
        let m = Milestone {
            source: Source::Journal { seq: 42 },
            ..milestone()
        };
        let text = serde_json::to_string(&m).unwrap();
        assert!(text.contains("\"seq\":42"), "{text}");
        assert_eq!(serde_json::from_str::<Milestone>(&text).unwrap(), m);
    }

    #[test]
    fn a_milestone_with_nothing_to_say_is_refused() {
        let mut m = milestone();
        m.title = "   ".into();
        assert!(m.check().is_err());

        let mut m = milestone();
        m.id = "".into();
        assert!(m.check().is_err());
    }

    /// The structural part of "a commit is not a milestone". There is no kind that means
    /// nothing in particular, so a caller has to make a claim about what was achieved.
    #[test]
    fn there_is_no_catch_all_achievement_kind() {
        for text in ["\"other\"", "\"commit\"", "\"misc\"", "\"\""] {
            assert!(
                serde_json::from_str::<Achievement>(text).is_err(),
                "{text} should not be an achievement"
            );
        }
    }

    /// And no anonymous source, so nothing can be recorded without somebody behind it.
    #[test]
    fn a_milestone_cannot_come_from_nobody() {
        assert!(serde_json::from_str::<Source>("\"anonymous\"").is_err());
        assert!(serde_json::from_str::<Source>("\"git\"").is_err());
        assert!(serde_json::from_str::<Source>("\"jj\"").is_ok());
    }

    /// A suggestion is a different type from an accepted milestone, so nothing can slip from
    /// one file into the other by deserialising.
    #[test]
    fn a_suggestion_is_not_a_milestone() {
        let s = Suggestion {
            project: id(),
            at: 1,
            title: "maybe this counts".into(),
            detail: None,
            evidence: Some("commit def456".into()),
            because: "the test suite went from red to green".into(),
        };
        let text = serde_json::to_string(&s).unwrap();
        assert!(
            serde_json::from_str::<Milestone>(&text).is_err(),
            "a suggestion must not load as history"
        );
    }
}
