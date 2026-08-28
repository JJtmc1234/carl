//! A lead reading the work it was given, and handing it to one of its own agents.
//!
//! The step below `objective`. Carl turns what JJ asked for into an objective for a lead, and
//! this turns that objective into a task for somebody who will actually do it. Without it an
//! objective reached Adrian and stopped: he held a task, Iris and Evan sat idle, and the army
//! looked busy from the outside while nothing moved.
//!
//! **Which agent is a judgement, so the lead makes it.** Nothing here reads the words and
//! guesses. That is the one question at this step that needs a model, and it is the reason a
//! lead is an agent rather than a routing table.
//!
//! **What the lead may do with the answer is not a judgement, so it does not make it.** The
//! name comes back and is checked against the organisation before anything is written. Adrian
//! naming Nora is refused here, not discouraged in a prompt, because a boundary somebody can
//! talk their way past is not one.

use super::words;
use crate::army::org;
use crate::army::task::{Task, Verification};
use crate::{Error, Result};

/// What a lead decided to do with the work it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandedOn {
    /// The agent who now owns it.
    pub agent: String,
    /// The task as written for that agent, which is not the words the lead was given.
    pub goal: String,
    /// What must be observable before it is done.
    pub must: Vec<String>,
}

/// The headings a lead is asked for, in the order they are expected.
const HEADINGS: [&str; 3] = ["AGENT", "TASK", "DONE WHEN"];

/// Reads the answer, without deciding whether the lead may have it.
///
/// Parsing and permission are kept apart so a change to the wording can never quietly widen
/// what a lead is allowed to reach.
pub fn read_choice(said: &str) -> HandedOn {
    let mut parts = [String::new(), String::new(), String::new()];
    let mut current: Option<usize> = None;

    for line in said.lines() {
        match heading_on(line) {
            Some(which) => current = Some(which),
            None => {
                if let Some(which) = current {
                    parts[which].push_str(line);
                    parts[which].push('\n');
                }
            }
        }
    }

    HandedOn {
        // First word only. A lead that answers "iris, since this is triage" has still named
        // Iris, and refusing that would be refusing a right answer over punctuation.
        agent: parts[0]
            .split(|c: char| !c.is_ascii_alphanumeric())
            .find(|w| !w.is_empty())
            .unwrap_or("")
            .to_lowercase(),
        goal: parts[1].trim().to_string(),
        must: parts[2]
            .lines()
            .map(|l| l.trim_start_matches(['-', '*', ' ']).trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    }
}

fn heading_on(line: &str) -> Option<usize> {
    let trimmed = line.trim().trim_end_matches(':').to_uppercase();
    HEADINGS.iter().position(|h| trimmed == *h)
}

/// The question a lead is asked, listing only the agents it actually has.
///
/// Built from the table, so an agent added to `org.rs` is offered without anybody editing this
/// and one removed stops being offered.
pub fn ask_which_agent(lead: &str, work: &str) -> Result<String> {
    let below = org::reports_of(lead);
    if below.is_empty() {
        return Err(Error::Refused(format!(
            "{lead} has nobody to hand work to, so this cannot go any further down"
        )));
    }
    let names: Vec<String> = below
        .iter()
        .map(|a| format!("  {} - {}", a.name, a.remit))
        .collect();

    Ok(format!(
        "You have been given this to get done:\n\n{work}\n\nHand it to exactly one of your \
         people. These are the only agents you may hand work to:\n\n{}\n\nAnswer in this form \
         and nothing else:\n\nAGENT:\n<one name from the list above>\n\nTASK:\n<the task written \
         for that agent, with the context they need so they do not have to guess. Say what to \
         achieve rather than how to do it>\n\nDONE WHEN:\n- <something observable>\n- <something \
         observable>\n\n{}",
        names.join("\n"),
        words::CONDITIONS
    ))
}

/// Turns a lead's answer into a real task for one of its agents, or refuses it.
///
/// `parent` is the task the lead is holding, so the new one hangs off it and the line back up
/// stays walkable. Losing that is how a subtask becomes an orphan nobody reviews.
///
/// **Writes nothing.** It builds the task and says whether the lead may have it. Recording the
/// handover is the board's, and it was done here as well at first: the board then read the
/// journal, found the task already delegated, and refused the very handover it had been given.
/// One writer, or the two come apart.
pub fn hand_on(lead: &str, parent: &Task, chosen: &HandedOn) -> Result<Task> {
    if chosen.agent.is_empty() {
        return Err(Error::Refused(format!(
            "{lead} named nobody. Work with no owner is work nobody is doing."
        )));
    }

    // The same rule the rest of the chain uses. There is no second, looser answer here about
    // who a lead may reach.
    if !org::may_delegate(lead, &chosen.agent) {
        let mine: Vec<&str> = org::reports_of(lead).iter().map(|a| a.name).collect();
        return Err(Error::Refused(format!(
            "{lead} cannot hand work to {}. {lead}'s people are: {}",
            chosen.agent,
            match mine.is_empty() {
                true => "nobody".to_string(),
                false => mine.join(", "),
            }
        )));
    }

    if chosen.goal.trim().is_empty() {
        return Err(Error::Refused(format!(
            "{lead} sent back an empty task, and nobody can do, review or refuse one of those"
        )));
    }

    let verification = Verification::of(chosen.must.clone())?;
    Task::split_from(parent, lead, &chosen.agent, &chosen.goal, verification)
}

#[cfg(test)]
mod tests;
