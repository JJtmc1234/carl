//! Letting a person put a given up agent back in the queue.
//!
//! The supervisor gives up on purpose. Six starts that did not stick means starting it again is
//! not the fix, and a loop that keeps trying forever burns money and buries the reason in noise.
//! `wake` refuses a degraded agent for exactly that reason and the refusal is right.
//!
//! What was missing is the other half. Nothing could undo it. On 2026 08 28 a transient failure
//! took all ten agents to degraded within five seconds of each other, and the army stayed down
//! for twenty one hours because no command existed to say the cause had been looked at. A state
//! only a human can leave, with no way for a human to leave it, is a dead end rather than a
//! safeguard.
//!
//! So this is deliberate, explicit, and says what it is not doing. It clears the verdict. It
//! does not diagnose anything, it does not start a process, and it does not promise the agent
//! will come up. The supervisor's ordinary policy decides that on its next pass, which is the
//! point: one way to start an agent, not two.

use std::path::Path;

use crate::army::event::{Because, Event, Journal};
use crate::army::personnel::Personnel;
use crate::army::runtime::{Lifecycle, Roll};
use crate::{Error, Result};

/// Written as the actor, because the supervisor did not decide this and should not appear to.
const ACTOR: &str = "jj";

/// What happened to one agent that was asked to be revived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revived {
    /// It was given up on and is now back in the ordinary queue.
    Cleared { was: String },
    /// It was not given up on, so there was no verdict to clear, but the recorded conversation
    /// was abandoned because that was asked for separately.
    SessionAbandoned,
    /// It was not given up on, so there was nothing to do.
    NotGivenUp(&'static str),
    /// Nothing has ever run it, so there is no verdict to undo.
    NoRecord,
}

/// Clears the given up verdict on one agent so the supervisor will consider it again.
///
/// Refuses nothing and pretends nothing. An agent that was not degraded comes back as
/// `NotGivenUp`, because reporting "revived" for an agent that was already fine would teach a
/// person that the command always works.
///
/// `now` is passed in rather than read here, the same way the supervisor takes its clock, so a
/// test can say what time it is instead of racing one.
///
/// `fresh` abandons the recorded conversation so the next start begins a new one. It defaults
/// off, because a session that is fine is continuity worth keeping and an agent that lost its
/// conversation still has its memory folder. It exists because a recorded session can name a
/// conversation that no longer exists, and then every resume fails the same way forever: the
/// supervisor gives up, a person revives it, and it gives up again on the next pass. Keeping a
/// dead session is not continuity, it is a loop.
pub fn one(home: &Path, name: &str, fresh: bool, now: u64) -> Result<Revived> {
    let agent = crate::army::org::require(name)?;
    let people = Personnel::open(home)?;
    let Some(folder) = people.get(agent.name) else {
        return Err(Error::Refused(format!(
            "{} has no folder, so there is nothing to revive. `carl army enlist {}` first",
            agent.name, agent.name
        )));
    };
    let Some(identity) = &folder.identity else {
        return Err(Error::Refused(format!(
            "{} has no identity, so no runtime record can belong to it",
            agent.name
        )));
    };

    let mut roll = Roll::open(home)?;
    let Some(mut record) = roll.get(&identity.id).cloned() else {
        return Ok(Revived::NoRecord);
    };

    // Abandoning a conversation known to be dead is its own act and does not depend on the
    // verdict. An agent revived once already sits in the ordinary queue with the same dead
    // session, and without this there was no way to clear it.
    let running = matches!(record.lifecycle, Lifecycle::Running { .. });

    let was = match &record.lifecycle {
        Lifecycle::Degraded { why } => Some(why.clone()),
        Lifecycle::Running { .. } => None,
        Lifecycle::Asleep { .. } => None,
        Lifecycle::Never => None,
        Lifecycle::Exited { .. } | Lifecycle::Stopped { .. } => None,
    };

    // A running agent is never touched, whatever was asked for. Pulling the session out from
    // under a live process is the failure `wake` documents: the next pass sees a record with no
    // conversation, starts a second one, and drops the first.
    if running {
        return Ok(Revived::NotGivenUp("it is running"));
    }

    if was.is_none() {
        if !fresh || record.session.is_none() {
            return Ok(Revived::NotGivenUp(match record.lifecycle {
                Lifecycle::Asleep { .. } => "it is asleep, which is not the same thing",
                Lifecycle::Never => "it has never been started",
                _ => "it is already in the ordinary queue",
            }));
        }
        if let Some(old) = record.session.take() {
            record.abandoned.push(old);
        }
        roll.save(home, record)?;
        return Ok(Revived::SessionAbandoned);
    }
    let was = was.expect("checked above");

    let mut journal = Journal::open(people.journal_path())?;
    journal.append(
        ACTOR,
        Event::AgentWoken {
            agent: identity.id.clone(),
            name: record.name.clone(),
            // JJ is the only one who can do this, and Lead is the closest honest reason
            // the journal has for "a person asked for it".
            because: Because::Lead {
                who: ACTOR.to_string(),
            },
        },
    )?;

    // Back to what a process that ended leaves behind, which is the same state `wake` produces.
    // Starting it here would be a second way to start an agent, and two ways to do one thing is
    // two backoff counters that disagree.
    record.lifecycle = Lifecycle::Exited {
        code: None,
        at: now,
    };
    record.attempts = 0;

    // Moved to abandoned rather than dropped, because what an agent was in the middle of is
    // worth being able to find later even when it can no longer be resumed.
    if fresh && let Some(old) = record.session.take() {
        record.abandoned.push(old);
    }
    roll.save(home, record)?;

    Ok(Revived::Cleared { was })
}

#[cfg(test)]
mod tests;
