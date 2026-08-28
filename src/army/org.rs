//! Who exists, what rank they hold, and who they answer to.
//!
//! The organisation, as JJ settled it in Week 2 of the Protocol Z course:
//!
//! ```text
//!   JJ                          the human, and the only authority that is not delegated
//!    |
//!   Carl                        chief executive, management only, never implements
//!    |
//!    +-- Adrian                 engineering
//!    |     +-- Iris             writes the GitHub issues
//!    |     +-- Evan             fixes them
//!    |
//!    +-- Mason                  Factorio
//!    |     +-- Nora             JJtorio developer
//!    |
//!    +-- Olivia                 operations
//!    |     +-- Miles            email and communications
//!    |
//!    +-- Serena                 security, with no agents under her yet
//!    |
//!    +-- Rowan                  research, with no agents under him yet
//! ```
//!
//! **Mason answers to Carl rather than to Adrian.** Factorio was a sub department of coding
//! while coding was the only department. It is a department in its own right now, and leaving
//! it nested would have put two leads between Carl and the person doing the work for no reason
//! anybody could state.
//!
//! **Serena and Rowan lead nothing yet, and that is deliberate.** A department with no agents
//! is a decision about where future work goes, recorded before there is any. Inventing workers
//! to fill them would mean founding agents nobody has a job for.
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
        name: "iris",
        display: "Iris",
        rank: Rank::Worker,
        reports_to: Some("adrian"),
        remit: "Writes the GitHub issues. Reads the repositories and reports what is actually \
                wrong, with the file, the mechanism and how she found it. Reports rather than \
                repairs: she changes no code, which is what keeps her judgement worth having.",
    },
    Agent {
        name: "evan",
        display: "Evan",
        rank: Rank::Worker,
        reports_to: Some("adrian"),
        remit: "Fixes the issues Iris writes, in the order Adrian gives them. Proves each fix \
                with a test that fails without it. Deletes and edits nothing without asking \
                JJ first.",
    },
    Agent {
        name: "mason",
        display: "Mason",
        rank: Rank::Lead,
        reports_to: Some("carl"),
        remit: "Head of the Factorio department. Breaks Carl's objective into tasks and \
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
    Agent {
        name: "olivia",
        display: "Olivia",
        rank: Rank::Lead,
        reports_to: Some("carl"),
        remit: "Head of operations. Owns what reaches JJ from outside: mail, messages and \
                anything else that arrives without being asked for. Manages and reviews rather \
                than writing the replies.",
    },
    Agent {
        name: "miles",
        display: "Miles",
        rank: Rank::Worker,
        reports_to: Some("olivia"),
        remit: "Email and communications. Reads the inbox, says which messages matter and \
                why, and drafts replies. Sends nothing and deletes nothing until JJ has said \
                so.",
    },
    Agent {
        name: "serena",
        display: "Serena",
        rank: Rank::Lead,
        reports_to: Some("carl"),
        remit: "Head of security. Leads nobody yet, and holds the department so that security \
                work has somewhere to go rather than being spread across whoever noticed it.",
    },
    Agent {
        name: "rowan",
        display: "Rowan",
        rank: Rank::Lead,
        reports_to: Some("carl"),
        remit: "Head of research. Leads nobody yet, and holds the department for the same \
                reason Serena holds hers.",
    },
];

/// The organisation, written for a prompt.
///
/// One place that renders the table into words, so the chain brief and the conversational brief
/// cannot come to describe different armies. Names the rank and the reporting line for every
/// agent, and states plainly what Carl may and may not do with that knowledge.
pub fn as_brief() -> String {
    let mut out = String::from(
        "THE ARMY. You are Carl, the chief executive. These agents exist and work for you:\n",
    );
    for a in ORG {
        if a.rank == Rank::Human {
            continue;
        }
        let under = match a.reports_to {
            Some(boss) => format!(", under {boss}"),
            None => String::new(),
        };
        out.push_str(&format!(
            "  {} ({}{}): {}\n",
            a.name, a.rank, under, a.remit
        ));
    }
    out.push_str(
        "\nYou hand work only to the agents directly below you, which is your leads. You never \
         write, review or rewrite the work itself, and you hold no tools for it, which is \
         deliberate rather than a fault. If JJ asks about an agent who is not one of your direct \
         reports, say whose they are and offer to ask that lead. Never say you have not heard of \
         somebody who is on this list.",
    );
    out
}

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
    fn every_chain_reaches_jj_through_its_own_lead() {
        let chain: Vec<&str> = chain_to_root("nora").iter().map(|a| a.name).collect();
        assert_eq!(chain, vec!["nora", "mason", "carl", "jj"]);

        // Factorio used to hang under coding, which put two leads between Carl and the person
        // doing the work. It is its own department now.
        let chain: Vec<&str> = chain_to_root("evan").iter().map(|a| a.name).collect();
        assert_eq!(chain, vec!["evan", "adrian", "carl", "jj"]);

        let chain: Vec<&str> = chain_to_root("miles").iter().map(|a| a.name).collect();
        assert_eq!(chain, vec!["miles", "olivia", "carl", "jj"]);
    }

    /// Every agent has to reach JJ, or somebody answers to nobody without being JJ.
    #[test]
    fn nobody_is_stranded_off_the_chain() {
        for a in everyone() {
            let chain = chain_to_root(a.name);
            assert_eq!(
                chain.last().map(|top| top.name),
                Some("jj"),
                "{} does not reach JJ: {:?}",
                a.name,
                chain.iter().map(|c| c.name).collect::<Vec<_>>()
            );
        }
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
    fn carl_cannot_reach_past_a_lead() {
        assert!(may_delegate("carl", "adrian"));
        assert!(may_delegate("carl", "mason"), "mason is his own lead now");
        assert!(may_delegate("carl", "olivia"));
        assert!(!may_delegate("carl", "nora"));
        assert!(!may_delegate("carl", "evan"));
        assert!(!may_delegate("carl", "miles"));

        let err = check_delegation("carl", "nora").unwrap_err().to_string();
        assert!(err.contains("cannot hand work straight to"), "{err}");
        assert!(err.contains("mason"), "and says who to ask instead: {err}");

        let err = check_delegation("adrian", "miles").unwrap_err().to_string();
        assert!(
            err.contains("olivia"),
            "one lead cannot take another's worker: {err}"
        );
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
            vec!["iris", "evan"]
        );
        assert_eq!(
            reports_of("olivia")
                .iter()
                .map(|a| a.name)
                .collect::<Vec<_>>(),
            vec!["miles"]
        );
        assert!(reports_of("nora").is_empty(), "nora leads nobody");

        // Held so that the work has somewhere to go, before there is any. A lead with nobody
        // under them is a decision that has been recorded, not an oversight.
        assert!(reports_of("serena").is_empty());
        assert!(reports_of("rowan").is_empty());
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

    /// The shape JJ specified: exactly three operational layers, with JJ outside them.
    ///
    /// Written as one test rather than spread through the file because the whole point is that
    /// it is checkable at a glance. If somebody adds a sub department this fails, which is the
    /// intent: deeper hierarchy is a decision, not something that arrives by accident.
    #[test]
    fn the_army_is_exactly_three_layers_deep_below_jj() {
        for a in everyone() {
            let depth = chain_to_root(a.name).len();
            let allowed = match a.rank {
                Rank::Human => 1,  // JJ, outside the army
                Rank::Chief => 2,  // Carl under JJ
                Rank::Lead => 3,   // a lead under Carl
                Rank::Worker => 4, // an agent under its lead
            };
            assert_eq!(
                depth, allowed,
                "{} is {} steps from JJ, which is not where a {} sits",
                a.name, depth, a.rank
            );
        }
    }

    /// Carl manages leads. Every worker belongs to exactly one lead and to no other.
    #[test]
    fn carl_reaches_every_lead_and_no_worker() {
        for a in everyone() {
            match a.rank {
                Rank::Lead => assert!(
                    may_delegate("carl", a.name),
                    "carl cannot reach his own lead {}",
                    a.name
                ),
                Rank::Worker => assert!(
                    !may_delegate("carl", a.name),
                    "carl reaches past a lead to {}",
                    a.name
                ),
                _ => {}
            }
        }
    }

    /// A lead may hand work to its own agents and to nobody else's.
    #[test]
    fn a_lead_cannot_take_another_leads_agent() {
        for lead in everyone().iter().filter(|a| a.rank == Rank::Lead) {
            for worker in everyone().iter().filter(|a| a.rank == Rank::Worker) {
                let mine = worker.reports_to == Some(lead.name);
                assert_eq!(
                    may_delegate(lead.name, worker.name),
                    mine,
                    "{} and {} : reports_to is {:?}",
                    lead.name,
                    worker.name,
                    worker.reports_to
                );
            }
        }
    }

    /// JJ is not in the army. Nothing may be handed to him and he holds no folder.
    #[test]
    fn jj_sits_outside_the_operational_army() {
        let jj = require("jj").unwrap();
        assert!(jj.is_root(), "JJ answers to nobody");
        assert_eq!(jj.rank, Rank::Human);
        assert_eq!(
            reports_of("jj").iter().map(|a| a.name).collect::<Vec<_>>(),
            vec!["carl"],
            "and only Carl is under him"
        );
        for a in everyone() {
            assert!(
                !may_delegate(a.name, "jj"),
                "{} can hand work to JJ, who is not in the army",
                a.name
            );
        }
    }

    /// JJ asked Carl how Miles was getting on and Carl said he did not know who Miles is.
    ///
    /// He was right about what he had been told. The conversational brief described a general
    /// assistant and never mentioned an army. The chain brief had the chart, but that is a
    /// different prompt, so the Carl JJ actually talks to had never seen it.
    #[test]
    fn the_brief_names_every_agent_and_who_they_answer_to() {
        let brief = as_brief();
        for a in everyone() {
            if a.rank == Rank::Human {
                continue;
            }
            assert!(brief.contains(a.name), "{} is not in the brief", a.name);
        }
        assert!(
            brief.contains("miles"),
            "miles specifically, which is the case that failed"
        );
        assert!(
            brief.contains("under olivia"),
            "and who Miles answers to, so Carl can point at her"
        );
    }

    /// JJ is not an agent and must not appear in a list headed "these agents work for you".
    #[test]
    fn the_brief_leaves_jj_out_of_the_army() {
        let brief = as_brief();
        for line in brief.lines().filter(|l| l.starts_with("  ")) {
            assert!(
                !line.starts_with("  jj "),
                "JJ is listed as an agent: {line}"
            );
        }
    }

    /// Handing Carl the chart must not read as permission to use all of it.
    #[test]
    fn the_brief_repeats_the_rule_that_goes_with_the_chart() {
        let brief = as_brief();
        assert!(brief.contains("only to the agents directly below you"));
        assert!(brief.contains("never write, review or rewrite"));
        assert!(
            brief.contains("Never say you have not heard of somebody"),
            "the actual failure is not named, so it can come back"
        );
    }

    /// The matrix JJ wrote out, checked pair by pair rather than by rule.
    ///
    /// The other tests check the shape in general terms. This one is the literal list, so if
    /// somebody changes a reporting line the failure names the exact pair that broke rather
    /// than a property somebody then has to interpret.
    #[test]
    fn the_delegation_matrix_jj_specified_holds() {
        let allowed = [
            ("carl", "adrian"),
            ("carl", "mason"),
            ("carl", "olivia"),
            ("carl", "serena"),
            ("carl", "rowan"),
            ("adrian", "iris"),
            ("adrian", "evan"),
            ("mason", "nora"),
            ("olivia", "miles"),
        ];
        for (from, to) in allowed {
            assert!(
                may_delegate(from, to),
                "{from} to {to} should be allowed and is not"
            );
        }

        let refused = [
            // Carl reaching past a lead to an ordinary agent.
            ("carl", "iris"),
            ("carl", "evan"),
            ("carl", "nora"),
            ("carl", "miles"),
            // One lead taking another lead's agent.
            ("adrian", "nora"),
            ("adrian", "miles"),
            ("mason", "iris"),
            ("mason", "evan"),
            ("mason", "miles"),
            ("olivia", "iris"),
            ("olivia", "nora"),
            // A lead handing to another lead, which is Carl's to do.
            ("adrian", "mason"),
            ("mason", "olivia"),
            ("olivia", "serena"),
            // Upward, and to JJ, who is not in the army.
            ("nora", "mason"),
            ("iris", "adrian"),
            ("miles", "olivia"),
            ("carl", "jj"),
            ("adrian", "jj"),
        ];
        for (from, to) in refused {
            assert!(
                !may_delegate(from, to),
                "{from} to {to} should be refused and is allowed"
            );
        }
    }
}
