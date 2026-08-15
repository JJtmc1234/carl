//! What the panel may ask for, and what happens to the record when it does.
//!
//! **There is no actor field, and that is the security design.** A command cannot say who sent
//! it. Everything arriving on the socket is attributed to JJ, because the socket is inside a
//! directory only JJ can traverse and is itself owner only, so the kernel has already answered
//! the question before the first byte is parsed. A field would be worse than useless: it would
//! be a place for a caller to write "mason" and be believed.
//!
//! That is also why no agent can escalate through here. Agents run under bubblewrap with no
//! home directory bound, so the socket path does not exist for them. It is not a check they
//! could pass or fail. There is nothing there.
//!
//! **JJ's authority and a security bypass are different things, and the record keeps them
//! apart.** JJ may hand work straight to Nora over Mason's head, and the chain rules that stop
//! Carl doing it do not apply to him. But it is written down as `Intervened`, never as an
//! ordinary `Delegated` from Mason, because the difference between "Mason reassigned her" and
//! "JJ reassigned her over Mason's head" is the entire reason for having a chain. A record that
//! blurred the two would still be a record and would answer the one question wrong.

use serde::{Deserialize, Serialize};

use crate::army::event::{Event, Intervention, Journal};
use crate::army::org;
use crate::{Error, Result};

/// Something JJ wants done.
///
/// `Say` and `Objective` go through Carl the ordinary way. The four `Jj*` commands reach past
/// him, and are named so that reading the enum tells you which is which.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// Tagged `kind` rather than `command`, because the envelope field that carries this is already
// called `command`, and `{"command": {"command": ...}}` reads like a mistake even when it is not.
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PanelCommand {
    /// An ordinary message to Carl, in the same conversation every other surface uses.
    Say { text: String },
    /// A new objective for Carl. Recorded, because an objective outlives the sentence.
    Objective { text: String },
    /// An answer to something Carl put to JJ, tied to the sequence that asked.
    Answer { seq: u64, text: String },
    /// Read one agent's folder and recent record. Changes nothing.
    Inspect { agent: String },
    /// A message straight to one agent, going around its lead.
    JjMessage { agent: String, text: String },
    /// A standing instruction that overrides what the agent's lead told it.
    JjInstruct { agent: String, instruction: String },
    /// Stop what an agent is doing, where it stands.
    JjStop { agent: String, why: String },
    /// Stop it and put something else in its place.
    JjReplace {
        agent: String,
        goal: String,
        why: String,
    },
}

impl PanelCommand {
    /// The agent this touches directly, if any.
    pub fn agent(&self) -> Option<&str> {
        match self {
            PanelCommand::JjMessage { agent, .. }
            | PanelCommand::JjInstruct { agent, .. }
            | PanelCommand::JjStop { agent, .. }
            | PanelCommand::JjReplace { agent, .. }
            | PanelCommand::Inspect { agent } => Some(agent),
            _ => None,
        }
    }

    /// Whether carrying this out reaches past the chain of command.
    pub fn is_intervention(&self) -> bool {
        matches!(
            self,
            PanelCommand::JjMessage { .. }
                | PanelCommand::JjInstruct { .. }
                | PanelCommand::JjStop { .. }
                | PanelCommand::JjReplace { .. }
        )
    }

    /// Refuses a command that could not be carried out, before anything is written down.
    ///
    /// Checked here rather than at the point of use so a malformed command never reaches the
    /// journal. A record of an attempt that was never coherent enough to attempt is noise, and
    /// the record is worth keeping clean.
    pub fn check(&self) -> Result<()> {
        if let Some(agent) = self.agent() {
            let agent = org::require(agent)?;
            if agent.rank == org::Rank::Human {
                return Err(Error::Refused(format!(
                    "{} is not an agent, so there is nobody to send this to",
                    agent.name
                )));
            }
        }

        let empty = |what: &str, s: &str| {
            s.trim()
                .is_empty()
                .then(|| Error::Refused(format!("{what} cannot be empty")))
        };
        let bad = match self {
            PanelCommand::Say { text } => empty("a message", text),
            PanelCommand::Objective { text } => empty("an objective", text),
            PanelCommand::Answer { seq, text } => {
                if *seq == 0 {
                    Some(Error::Refused(
                        "an answer has to name the sequence it answers, and 0 is not one".into(),
                    ))
                } else {
                    empty("an answer", text)
                }
            }
            PanelCommand::JjMessage { text, .. } => empty("a message", text),
            PanelCommand::JjInstruct { instruction, .. } => empty("an instruction", instruction),
            PanelCommand::JjStop { why, .. } => empty("a reason for stopping somebody", why),
            PanelCommand::JjReplace { goal, why, .. } => {
                empty("a goal", goal).or_else(|| empty("a reason for replacing a task", why))
            }
            PanelCommand::Inspect { .. } => None,
        };

        match bad {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// What was written down, so the caller can report it and a test can check it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// The sequence the intervention itself landed at.
    pub seq: u64,
    /// Who was told, in the order they were told. Carl first, then the agent's lead.
    pub told: Vec<String>,
}

/// Writes an intervention down and tells the people whose picture of the world it just changed.
///
/// The order matters. The intervention is recorded first, and the notifications point back at
/// its sequence rather than repeating what it said, so a notification can never come to
/// disagree with the thing it notifies about.
///
/// Carl is always told, because he is accountable for the army and an army that changed under
/// him without his knowing is one he cannot answer for. The agent's lead is told when there is
/// one and it is not already Carl, because the lead is the person who thought they knew what
/// their report was doing.
pub fn record(journal: &mut Journal, what: Intervention) -> Result<Recorded> {
    let seq = journal
        .append("jj", Event::Intervened { what: what.clone() })?
        .seq;

    let mut told = Vec::new();
    let tell = |journal: &mut Journal, who: &str, told: &mut Vec<String>| -> Result<()> {
        if told.iter().any(|t| t == who) {
            return Ok(());
        }
        journal.append(
            "jj",
            Event::Notified {
                who: who.to_string(),
                about: seq,
            },
        )?;
        told.push(who.to_string());
        Ok(())
    };

    tell(journal, "carl", &mut told)?;
    if let Some(agent) = what.agent()
        && let Ok(agent) = org::require(agent)
        && let Some(lead) = agent.reports_to
        // Carl's own lead is JJ, and telling JJ what JJ just did is noise in a record whose
        // value is that everything in it is worth reading.
        && lead != "jj"
    {
        tell(journal, lead, &mut told)?;
    }

    Ok(Recorded { seq, told })
}
