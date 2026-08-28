//! One pass down the chain, moving whatever is stuck.
//!
//! The thing that was missing. Every piece existed and nothing joined them: an objective was
//! recorded and Carl was told, a lead could be handed work, an agent could be given a task, and
//! no code ever walked from one to the next. So the army sat with ten processes alive and
//! nothing to do, which looks identical to an army that is working hard.
//!
//! A pass is deliberately one step per task rather than a recursive drive to completion.
//! Anything that runs a whole campaign in one call is impossible to interrupt, impossible to
//! watch, and spends money in a shape nobody chose. This moves what is ready, says what it did,
//! and returns.

use crate::army::board::Board;
use crate::army::org::{self, Rank};
use crate::army::task::Status;
use crate::{Error, Result};

/// What one pass did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Moved {
    /// Work handed from a lead to one of its agents, as `lead -> agent`.
    pub handed_on: Vec<(String, String)>,
    /// Things that could not move, and why. Kept rather than dropped: a pass that quietly does
    /// nothing is the state this whole module exists to end.
    pub stuck: Vec<String>,
}

impl Moved {
    pub fn nothing(&self) -> bool {
        self.handed_on.is_empty()
    }
}

/// Whatever a lead is holding that has not been passed on.
///
/// A lead holding work with no child task is the shape of a stalled chain, and it is the only
/// thing this pass acts on. A worker holding work is not stuck, it is working.
pub fn waiting_on_a_lead(board: &Board) -> Result<Vec<crate::army::task::Task>> {
    let all = board.tasks()?;
    let mut out = Vec::new();

    for task in &all {
        let Ok(owner) = org::require(&task.owner) else {
            continue;
        };
        if owner.rank != Rank::Lead {
            continue;
        }
        if matches!(task.status, Status::Accepted | Status::Abandoned) {
            continue;
        }
        // Already handed on, so the lead is waiting on somebody rather than sitting on it.
        if all.iter().any(|t| t.parent.as_ref() == Some(&task.id)) {
            continue;
        }
        out.push(task.clone());
    }
    Ok(out)
}

/// Everything submitted and waiting on whoever created it.
///
/// The other direction. Work goes down by being handed on and comes back up by being reviewed,
/// and until now only the first half existed: an agent could finish and the task sat submitted
/// with nobody looking at it, which is the same as not being finished.
///
/// The reviewer is always the agent who created the task. Not the rank above, and not whoever
/// is free: a task is reviewable by the person who asked for it, because they are the only one
/// who knows what they wanted.
pub fn waiting_on_review(board: &Board) -> Result<Vec<crate::army::task::Task>> {
    Ok(board
        .tasks()?
        .into_iter()
        .filter(|t| t.status == Status::Submitted)
        .collect())
}

/// Records one review, using a verdict somebody else obtained.
///
/// Accepting is deliberately not the default when an answer cannot be read. A reviewer who says
/// something unreadable has not approved anything, and treating that as approval is how work
/// nobody checked gets marked done.
pub fn review_one(
    board: &mut Board,
    people: Option<&mut crate::army::personnel::Personnel>,
    task: &crate::army::task::Task,
    said: &str,
) -> Result<(bool, String)> {
    let (verdict, why) = super::read_verdict(said);
    let accepted = matches!(verdict, super::Verdict::Accept);

    board.review(&task.created_by, &task.id, accepted, &why)?;

    // The worker's folder stops saying they are busy once the work is accepted. A folder that
    // keeps somebody occupied after their task is done is why nothing else gets handed to them.
    if accepted && let Some(people) = people {
        let now = crate::army::event::now();
        people.update_state(&task.owner, |s| s.put_down("accepted", now))?;
    }
    Ok((accepted, why))
}

/// Hands one lead's work to one of its agents, using an answer somebody else obtained.
///
/// The model call is the caller's, because this crate has no opinion about how a lead is asked.
/// That keeps the rule and the conversation apart: this decides whether the answer is allowed,
/// and it would decide the same thing whoever produced it.
pub fn hand_on_one(
    board: &mut Board,
    people: Option<&mut crate::army::personnel::Personnel>,
    lead: &str,
    parent: &crate::army::task::Task,
    said: &str,
) -> Result<(String, crate::army::task::Task)> {
    let chosen = super::assign::read_choice(said);
    let task = super::assign::hand_on(lead, parent, &chosen)?;

    // The board is the one writer. It records the handover and enforces the rules that go with
    // it, so nothing else appends a `Delegated` for the same task.
    board.delegate(lead, &task).map_err(|e| {
        Error::Refused(format!(
            "{lead} handed work to {} and the board refused it: {e}",
            chosen.agent
        ))
    })?;

    // And the agent's own folder, which is what survives a restart and what the panel reads to
    // say who is busy. Writing only the board left Nora holding a task in the record and
    // reading as idle on screen, which is two answers to one question.
    if let Some(people) = people {
        let now = crate::army::event::now();
        people.update_state(&chosen.agent, |s| s.take_up(&task.id, now))?;
    }

    Ok((chosen.agent, task))
}

#[cfg(test)]
mod tests;
