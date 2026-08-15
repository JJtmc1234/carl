//! Driving one real task down the chain and back up again.
//!
//! ```text
//!   JJ
//!    -> carl      turns it into an objective and hands it to Adrian
//!        -> adrian    decides it is Factorio work and hands it to Mason
//!            -> mason     writes one concrete task and reviews what comes back
//!                -> nora      does it, verifies it, submits it
//! ```
//!
//! Who exists, who may hand work to whom, and what a task is are all decided elsewhere. This
//! module owns only the part that needs a real model: what each agent is actually told, what
//! is done with what it says, whether a review passed, and when to stop trying.
//!
//! | this module | the shared foundations |
//! |---|---|
//! | the brief each agent runs with | `org::Agent`, who exists and what rank |
//! | what is sent at each step | `task::Task`, the governed unit and its states |
//! | reading a report and a verdict | `event::Journal`, what happened |
//! | the retry loop and when to give up | `task.attempts`, the counter it reads |
//!
//! One thing here is enforcement rather than prompting. Each agent's `claude` process starts
//! with a tool list chosen by rank, so Carl's process cannot edit a file whatever it decides
//! it would like to do. `org::check_may_implement` says the same thing and says it first, but
//! a rule that only exists as a check is a rule that holds until somebody forgets to call it.

mod drive;
mod handback;
mod run;
mod words;

pub use drive::campaign;
pub use handback::{Handback, Verdict, read_handback, read_must, read_verdict};
pub use run::{Chain, DEADLINE, MAX_ATTEMPTS, Next, Passage, after_review};

use super::org::{Agent, Rank, reports_of};

/// What the `claude` process for this rank is allowed to use.
///
/// The enforcement point for the rank rules, next to the check rather than instead of it. A
/// lead told in prose not to write code is a lead who writes code the moment the prose stops
/// being convenient. A lead started without an editor cannot.
///
/// Mason gets `Bash` and no editor, because reviewing means rerunning Nora's verification
/// rather than taking her word for it. That is a real gap, since a shell can write a file, and
/// it is the smallest grant that still lets a reviewer check rather than trust. Nothing in any
/// list raises privileges, and there is nowhere in `org.rs` to ask for that anyway.
pub fn tools_for(rank: Rank) -> Vec<String> {
    let names: &[&str] = match rank {
        // A chief with no tools cannot implement by accident, which is what the rank says.
        Rank::Human | Rank::Chief => &[],
        Rank::Lead => &["Read", "Grep", "Glob", "Bash"],
        Rank::Worker => &["Read", "Grep", "Glob", "Bash", "Write", "Edit"],
    };
    names.iter().map(|s| s.to_string()).collect()
}

/// What everybody in the chain is told, whatever their rank.
const STANDING: &str = "\
You are one named agent in a chain of command working for JJ. You speak only to the agent \
directly above you and the agents directly below you. Never address anybody else in the chain \
and never assume what they said.

Answer only what you were asked, and answer with the thing itself rather than a paragraph \
introducing it. Do not use dashes or semicolons. Be brief and be specific. Say plainly when \
you do not know something, because somebody below you will act on it as though it were \
checked.";

const CHIEF: &str = "\
You never write, review or rewrite the work itself. You have no tools and that is deliberate \
rather than an oversight. If you find yourself about to describe an implementation, stop and \
describe the outcome you want instead. What comes back up to you is a report, and your job \
with it is to tell JJ whether what he asked for happened.";

const LEAD: &str = "\
You manage and you review. You do not write the work. You decide what is asked for, who below \
you does it, and whether what comes back is actually done.

When you review, check rather than trust. Read what was changed and rerun the verification \
yourself. Accepting something you did not check is worse than doing it yourself, because then \
nobody checked it at all.

Nothing below you moves without you, and nothing above you hears from your people except \
through you.";

const WORKER: &str = "\
You own every implementation detail inside the task you were given, and nothing outside it. Do \
not ask how. Work it out, do it, and prove it.

You have tools to read and change files and to run commands. Use them. Run the verification \
the task asks for and read the real output rather than assuming it passed.

Before you ever call yourself blocked, read the relevant code, read the project context, read \
whatever documentation exists, and actually debug it. Only then report the blocker, with the \
exact error, what you tried, and what is still uncertain. Never guess at something you could \
have checked.

You never give yourself the next task. When you have reported, you stop and wait.";

/// The system prompt one agent's process starts with.
///
/// Built from the shared table rather than a second copy of it, so an agent added to `org.rs`
/// gets a correct brief without anything here changing.
pub fn brief_for(agent: &Agent) -> String {
    let rank = match agent.rank {
        Rank::Chief => CHIEF,
        Rank::Lead => LEAD,
        Rank::Worker | Rank::Human => WORKER,
    };

    let mut position = match agent.reports_to {
        Some(boss) => format!("You report to {boss}."),
        None => "You report to nobody.".to_string(),
    };
    let below = reports_of(agent.name);
    if below.is_empty() {
        position.push_str(" Nobody reports to you, so you hand work to nobody.");
    } else {
        position.push_str(&format!(
            " These agents report to you and are the only ones you may hand work to: {}.",
            below.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
        ));
    }

    format!(
        "You are {}. {}\n\n{position}\n\n{rank}\n\n{STANDING}",
        agent.display, agent.remit
    )
}

#[cfg(test)]
mod tests;
