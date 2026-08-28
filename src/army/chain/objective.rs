//! Carl reading an objective JJ recorded, and handing it to the lead it belongs to.
//!
//! The step that was missing. Setting an objective wrote it down and told Carl, and there it
//! stopped: he answered in conversation and nothing reached the army. Every objective about the
//! army came back to whoever was at the keyboard, which is the opposite of having one.
//!
//! **Which lead is a judgement, so Carl makes it.** Nothing here reads the words and guesses at
//! a department. That is the one question in this whole step that needs a model, and it is the
//! reason this is an agent rather than a rule.
//!
//! **What Carl may do with the answer is not a judgement, so he does not make it.** The name he
//! gives is checked against the organisation before anything is written. Carl naming Nora is
//! refused here, not discouraged in a prompt, because a lead he could talk his way past is not
//! a lead. That is also why this refuses rather than falling back to a default department: a
//! wrong owner chosen quietly is worse than an objective that visibly did not move.

use super::words;
use crate::army::event::{Journal, Record};
use crate::army::org;
use crate::army::task::{Task, Verification};
use crate::{Error, Result};

/// What Carl decided to do with one objective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandedDown {
    /// The lead who now owns it.
    pub lead: String,
    /// The objective as written for that lead, which is not the words JJ used.
    pub goal: String,
    /// What must be observable before it is done.
    pub must: Vec<String>,
}

/// The headings Carl is asked for, in the order they are expected.
const HEADINGS: [&str; 3] = ["LEAD", "OBJECTIVE", "DONE WHEN"];

/// Reads Carl's answer, without deciding anything about whether he may do it.
///
/// Parsing and permission are kept apart on purpose. This says what he asked for and `hand_down`
/// says whether he may have it, so a change to the wording can never quietly widen what he is
/// allowed to reach.
pub fn read_choice(said: &str) -> HandedDown {
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

    HandedDown {
        // First word only. A model that answers "mason, because this is Factorio work" has
        // still named Mason, and refusing that would be refusing a right answer over punctuation.
        lead: parts[0]
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

/// The question Carl is asked, listing the leads that actually exist.
///
/// Built from the table rather than written out, so a department added to `org.rs` is offered
/// without anybody remembering to come here, and one removed stops being offered.
pub fn ask_which_lead(objective: &str) -> String {
    let leads: Vec<String> = org::reports_of("carl")
        .iter()
        .map(|a| format!("  {} - {}", a.name, a.remit))
        .collect();

    format!(
        "JJ has set this objective:\n\n{objective}\n\nHand it to exactly one of your leads. \
         These are the only people you may hand work to:\n\n{}\n\nAnswer in this form and \
         nothing else:\n\nLEAD:\n<one name from the list above>\n\nOBJECTIVE:\n<the objective \
         written for that lead, saying what must be true when it is done and why JJ wants it. \
         Do not say how to build it and do not name files>\n\nDONE WHEN:\n- <something \
         observable>\n- <something observable>\n\n{}",
        leads.join("\n"),
        words::CONDITIONS
    )
}

/// Turns Carl's answer into a real delegation, or refuses it.
///
/// `objective_seq` is the journal sequence that recorded the objective, carried onto the task so
/// the two can never drift apart and so the panel can tell an objective that has been taken up
/// from one nobody has looked at.
pub fn hand_down(
    journal: &mut Journal,
    objective_seq: u64,
    chosen: &HandedDown,
) -> Result<(Record, Task)> {
    if chosen.lead.is_empty() {
        return Err(Error::Refused(
            "Carl named nobody. An objective with no owner is an objective nobody is doing.".into(),
        ));
    }

    // The check that makes the organisation real. `may_delegate` is the same rule the rest of
    // the chain uses, so there is no second, looser answer here about who Carl may reach.
    if !org::may_delegate("carl", &chosen.lead) {
        let leads: Vec<&str> = org::reports_of("carl").iter().map(|a| a.name).collect();
        return Err(Error::Refused(format!(
            "carl cannot hand an objective to {}. His leads are: {}",
            chosen.lead,
            leads.join(", ")
        )));
    }

    if chosen.goal.trim().is_empty() {
        return Err(Error::Refused(
            "the objective came back empty, and a lead cannot be handed nothing".into(),
        ));
    }

    let verification = Verification::of(chosen.must.clone())?;
    let task =
        Task::assign("carl", &chosen.lead, &chosen.goal, verification)?.answering(objective_seq);

    let record = journal.append(
        "carl",
        crate::army::event::Event::Delegated {
            task: task.id.clone(),
            to: chosen.lead.clone(),
            goal: task.goal.clone(),
            parent: None,
            must: task.verification.must.clone(),
            project: None,
            workspace: None,
            objective: Some(objective_seq),
        },
    )?;

    Ok((record, task))
}

#[cfg(test)]
mod tests;
