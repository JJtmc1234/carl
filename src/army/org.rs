//! Who exists, what rank they hold, and who they answer to.
//!
//! The first chain, exactly as JJ specified it:
//!
//! ```text
//!   JJ        the human, and the only authority that is not delegated
//!    |
//!   Carl      chief executive, management only, never implements
//!    |
//!   Adrian    coding department
//!    |
//!   Mason     Factorio sub department
//!    |
//!   Nora      JJtorio developer, owns the details inside her task
//! ```
//!
//! This is the authority on who exists. `roster.rs` is the older generic squad of unnamed
//! roles and is kept because the campaign path uses it, but nothing new should be built on it.
//! Everything in v1 hangs off the named agents here.
//!
//! Three rules are enforced by the types rather than left to a brief, because a brief is a
//! request and a type is not.
//!
//! **Delegation follows the chain.** An agent may hand work only to somebody who reports to
//! it. Carl cannot reach past Adrian to Nora, however obvious the shortcut looks, because a
//! chain that is skipped once stops being a chain.
//!
//! **Rank decides who may implement.** Carl never writes work at all. A lead manages and
//! reviews, and may implement only when an emergency is declared, which is a thing that gets
//! recorded rather than a mood.
//!
//! **Nobody is ever an administrator.** There is no way to express it, no flag to set, and no
//! rank that carries it. Something that cannot be represented cannot be granted by accident.

use std::fmt;

use crate::{Error, Result};

/// What an agent may do, by where it sits.
///
/// Serialisable because the panel renders it. The wire form is the lowercase name, so a reader
/// without this crate still gets something it can compare against a string.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Rank {
    /// A person. The only authority nobody delegated.
    Human,
    /// Chief executive. Delegates and decides, and writes none of the work.
    Chief,
    /// Leads a department or a sub department. Manages and reviews.
    Lead,
    /// Does the work, and owns how it is done inside the task assigned.
    Worker,
}

impl Rank {
    /// Whether this rank may carry out implementation work.
    ///
    /// A chief never may, whatever is happening, because a chief who implements is no longer
    /// delegating and the hierarchy is decoration. A lead may only under a declared emergency,
    /// and declaring one is an event somebody can later ask about.
    pub fn may_implement(self, emergency: bool) -> bool {
        match self {
            Rank::Human | Rank::Worker => true,
            Rank::Lead => emergency,
            Rank::Chief => false,
        }
    }

    /// Whether this rank reviews what those below it produce.
    pub fn reviews(self) -> bool {
        matches!(self, Rank::Human | Rank::Chief | Rank::Lead)
    }
}

impl fmt::Display for Rank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Rank::Human => "human",
            Rank::Chief => "chief",
            Rank::Lead => "lead",
            Rank::Worker => "worker",
        })
    }
}

/// One named member of the organisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Agent {
    /// Lowercase, and used as an identifier in events, tasks and filenames.
    pub name: &'static str,
    /// What people call them.
    pub display: &'static str,
    pub rank: Rank,
    /// Who this agent answers to. `None` only for JJ.
    pub reports_to: Option<&'static str>,
    /// What this agent is responsible for, in one line, for putting in a brief.
    pub remit: &'static str,
}

impl Agent {
    pub fn is_root(&self) -> bool {
        self.reports_to.is_none()
    }
}

/// The organisation. Small on purpose: this is the first chain and not the whole army.
static ORG: &[Agent] = &[
    Agent {
        name: "jj",
        display: "JJ",
        rank: Rank::Human,
        reports_to: None,
        remit: "The person this is all for. Decides what matters and settles anything Carl \
                cannot.",
    },
    Agent {
        name: "carl",
        display: "Carl",
        rank: Rank::Chief,
        reports_to: Some("jj"),
        remit: "Chief executive. Turns what JJ wants into objectives for departments and hands \
                them down. Writes none of the work and reviews none of the detail. Settles a \
                tie between departments only after each has put its case.",
    },
    Agent {
        name: "adrian",
        display: "Adrian",
        rank: Rank::Lead,
        reports_to: Some("carl"),
        remit: "Head of the coding department. Takes an objective from Carl, decides which sub \
                department it belongs to, and holds it to a standard. Manages and reviews \
                rather than writing code.",
    },
    Agent {
        name: "mason",
        display: "Mason",
        rank: Rank::Lead,
        reports_to: Some("adrian"),
        remit: "Head of the Factorio sub department. Breaks Adrian's objective into tasks and \
                assigns his developers one at a time. Reviews what comes back and decides \
                whether it is done.",
    },
    Agent {
        name: "nora",
        display: "Nora",
        rank: Rank::Worker,
        reports_to: Some("mason"),
        remit: "JJtorio developer. Owns every implementation detail inside the task she was \
                given, and owns nothing outside it.",
    },
];

pub fn everyone() -> &'static [Agent] {
    ORG
}

pub fn find(name: &str) -> Option<&'static Agent> {
    ORG.iter().find(|a| a.name == name)
}

/// The agent named, or a refusal that lists who exists.
///
/// Used at every boundary rather than `find`, because a typo in an agent name otherwise
/// becomes a silently skipped step rather than an error.
pub fn require(name: &str) -> Result<&'static Agent> {
    find(name).ok_or_else(|| {
        Error::Refused(format!(
            "there is no agent called {name}. The organisation is: {}",
            ORG.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
        ))
    })
}

/// Everybody who reports directly to this agent.
pub fn reports_of(name: &str) -> Vec<&'static Agent> {
    ORG.iter().filter(|a| a.reports_to == Some(name)).collect()
}

/// The chain from an agent up to JJ, starting with the agent itself.
///
/// Bounded rather than recursive without a limit. A cycle in the table would otherwise hang
/// whatever asked, and the table is written by hand.
pub fn chain_to_root(name: &str) -> Vec<&'static Agent> {
    let mut out = Vec::new();
    let mut at = find(name);

    while let Some(agent) = at {
        out.push(agent);
        if out.len() > ORG.len() {
            break;
        }
        at = agent.reports_to.and_then(find);
    }
    out
}

/// Whether `from` may hand work directly to `to`.
///
/// Only to a direct report, for v1. Carl reaching past Adrian to Nora is the shortcut that
/// looks harmless once and removes the point of having Adrian at all, so it is refused here
/// rather than discouraged in a brief.
pub fn may_delegate(from: &str, to: &str) -> bool {
    find(to).is_some_and(|t| t.reports_to == Some(from))
}

/// Refuses a delegation that does not follow the chain, and says what would.
pub fn check_delegation(from: &str, to: &str) -> Result<()> {
    let boss = require(from)?;
    let worker = require(to)?;

    if may_delegate(boss.name, worker.name) {
        return Ok(());
    }

    let route = match worker.reports_to {
        Some(head) => format!("{} answers to {head}, so ask {head}", worker.name),
        None => format!("{} answers to nobody", worker.name),
    };
    Err(Error::Refused(format!(
        "{} cannot hand work straight to {}. {route}.",
        boss.name, worker.name
    )))
}

/// Refuses implementation work by somebody whose rank forbids it.
pub fn check_may_implement(name: &str, emergency: bool) -> Result<()> {
    let agent = require(name)?;
    if agent.rank.may_implement(emergency) {
        return Ok(());
    }
    Err(Error::Refused(match agent.rank {
        Rank::Chief => format!(
            "{} is the chief executive and does not implement anything, in any circumstance. \
             Delegate it.",
            agent.name
        ),
        _ => format!(
            "{} is a {} and manages rather than implements. This needs an emergency to be \
             declared first, and that is recorded.",
            agent.name, agent.rank
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chain JJ specified, end to end.
    #[test]
    fn the_first_chain_is_exactly_what_was_asked_for() {
        let chain: Vec<&str> = chain_to_root("nora").iter().map(|a| a.name).collect();
        assert_eq!(chain, vec!["nora", "mason", "adrian", "carl", "jj"]);
    }

    #[test]
    fn everybody_but_jj_answers_to_somebody_real() {
        for a in everyone() {
            match a.reports_to {
                None => assert_eq!(a.name, "jj", "{} answers to nobody", a.name),
                Some(boss) => {
                    assert!(find(boss).is_some(), "{} answers to a ghost", a.name);
                }
            }
        }
    }

    /// The shortcut that looks harmless once and removes the point of having Adrian.
    #[test]
    fn carl_cannot_reach_past_adrian() {
        assert!(may_delegate("carl", "adrian"));
        assert!(!may_delegate("carl", "mason"));
        assert!(!may_delegate("carl", "nora"));

        let err = check_delegation("carl", "nora").unwrap_err().to_string();
        assert!(err.contains("cannot hand work straight to"), "{err}");
        assert!(err.contains("mason"), "and says who to ask instead: {err}");
    }

    #[test]
    fn work_flows_down_and_never_up() {
        assert!(may_delegate("mason", "nora"));
        assert!(
            !may_delegate("nora", "mason"),
            "a worker does not assign her lead"
        );
        assert!(!may_delegate("adrian", "carl"));
    }

    /// A chief who implements is not delegating, and the hierarchy is decoration.
    #[test]
    fn carl_never_implements_whatever_is_happening() {
        assert!(!Rank::Chief.may_implement(false));
        assert!(!Rank::Chief.may_implement(true), "not even in an emergency");

        let err = check_may_implement("carl", true).unwrap_err().to_string();
        assert!(err.contains("in any circumstance"), "{err}");
    }

    /// A lead manages, and may write code only when an emergency has been declared, which is
    /// a thing that gets recorded rather than a mood.
    #[test]
    fn a_lead_may_only_implement_in_an_emergency() {
        assert!(!Rank::Lead.may_implement(false));
        assert!(Rank::Lead.may_implement(true));

        assert!(check_may_implement("mason", false).is_err());
        assert!(check_may_implement("mason", true).is_ok());
    }

    #[test]
    fn a_worker_implements_by_default() {
        assert!(Rank::Worker.may_implement(false));
        assert!(check_may_implement("nora", false).is_ok());
    }

    /// A typo in a name must be an error rather than a silently skipped step.
    #[test]
    fn an_unknown_agent_is_refused_and_the_list_is_given() {
        let err = require("norah").unwrap_err().to_string();
        assert!(err.contains("no agent called norah"), "{err}");
        assert!(err.contains("nora"), "and lists who does exist: {err}");
    }

    #[test]
    fn reports_are_found_by_who_they_answer_to() {
        assert_eq!(
            reports_of("adrian")
                .iter()
                .map(|a| a.name)
                .collect::<Vec<_>>(),
            vec!["mason"]
        );
        assert!(reports_of("nora").is_empty(), "nora leads nobody yet");
    }

    /// Nothing anywhere may express administrator rights. Something that cannot be
    /// represented cannot be granted by accident.
    #[test]
    fn no_rank_carries_administrator_rights() {
        // Only the real code, not this module. The first version scanned the whole file and
        // failed on its own assertions, which named the very words it was banning. A test that
        // cannot pass is as useless as one that cannot fail, and it is the same mistake.
        let source = include_str!("org.rs");
        let code = source.split("#[cfg(test)]").next().unwrap_or(source);

        for word in [
            "sudo",
            "root_access",
            "admin_privileges",
            "is_admin",
            "elevate",
        ] {
            let mentions: Vec<&str> = code
                .lines()
                .filter(|l| l.contains(word))
                .filter(|l| {
                    let t = l.trim_start();
                    !t.starts_with("//") && !t.starts_with("///") && !t.starts_with("//!")
                })
                .collect();
            assert!(mentions.is_empty(), "{word} appears in code: {mentions:?}");
        }

        // And no rank grants it by another name, which is the way it would actually creep in.
        for rank in [Rank::Human, Rank::Chief, Rank::Lead, Rank::Worker] {
            let described = format!("{rank}");
            assert!(!described.contains("admin"), "{described}");
        }
    }

    /// The table is written by hand, so a cycle in it must not hang whatever walks it.
    #[test]
    fn walking_the_chain_always_terminates() {
        for a in everyone() {
            let chain = chain_to_root(a.name);
            assert!(chain.len() <= everyone().len(), "{} loops", a.name);
            assert_eq!(chain[0].name, a.name);
        }
    }
}
