//! Handing work one step down the chain, for real.
//!
//! The hole this fills was found by removing the thing that was papering over it. Carl had no
//! way to reach Olivia at all, so given the built in subagent tool he used that instead: a fresh
//! process told "you are Miles, do this", with no identity, no memory, no rank and no lead. JJ
//! reported it as delegation being broken and it was.
//!
//! Taking the subagent tool away on its own made it worse rather than better. Asked to handle a
//! school email, Carl said "this would normally go to Olivia, but no Olivia session is running
//! and I cannot start one, so I am carrying it myself", and did the work. A chief doing the work
//! is the exact thing his rank exists to prevent. Refusing the fake route without providing the
//! real one just moves the failure.
//!
//! So this is the real route. It goes through `org::check_delegation`, so the edge is the same
//! one the rest of the system enforces, and it runs the named agent through `Chain`, so the
//! agent that answers is the actual agent: its own folder, its own memory, its own rank's tools,
//! its own deadline.

use std::path::Path;
use std::time::Duration;

use crate::Result;
use crate::army::chain::Chain;
use crate::army::event::{Intervention, Journal};
use crate::panel::command;
use crate::army::org;
use crate::army::personnel::Personnel;

/// What came back from handing one piece of work down.
pub struct Handed {
    /// The agent that did it.
    pub to: String,
    /// What they said.
    pub said: String,
    /// Where it was written down, when the journal took it.
    pub seq: Option<u64>,
}

/// Hands one piece of work from one agent to one of its direct reports.
///
/// Refuses anything that is not an edge in the organisation, and says what the route would be.
/// `from` reaching past its own reports is the shortcut that removes the reason the middle agent
/// exists, which is why it is refused here rather than discouraged in a brief.
///
/// The work is recorded before the agent runs, not after. A handoff whose agent then crashed is
/// still a handoff that happened, and the record is how anybody finds out.
pub fn hand(
    home: &Path,
    program: &Path,
    from: &str,
    to: &str,
    work: &str,
    deadline: Duration,
) -> Result<Handed> {
    org::check_delegation(from, to)?;

    if work.trim().is_empty() {
        return Err(crate::Error::Refused(format!(
            "there is nothing to hand to {to}"
        )));
    }

    let people = Personnel::open(home)?;
    let journal_path = people.journal_path();
    let workdir = people.folder(to);

    // Written down first. An agent that dies mid answer still leaves a record that it was
    // asked, which is the difference between a task nobody finished and a task nobody knows
    // about.
    let seq = {
        let mut journal = Journal::open(&journal_path)?;
        command::record(
            &mut journal,
            Intervention::Message {
                to: to.to_string(),
                what: format!("[from {from}] {work}"),
            },
        )
        .ok()
        .map(|r| r.seq)
    };

    let mut chain = Chain::new(program, &workdir, &journal_path)?
        .deadline(deadline)
        .staffed_by(people);

    let said = chain.ask(to, work)?;

    Ok(Handed {
        to: to.to_string(),
        said,
        seq,
    })
}

#[cfg(test)]
mod tests;
