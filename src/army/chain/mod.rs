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

pub mod assign;
mod drive;
mod handback;
pub mod objective;
mod run;
mod staffing;
pub mod words;
pub mod work;

pub use drive::campaign;
pub use handback::{Handback, Verdict, read_handback, read_must, read_verdict};
pub use run::{Chain, DEADLINE, Next, Passage, after_review};
pub use staffing::{check_rule, outstanding};

/// Re-exported rather than redefined.
///
/// There were two of these for a day, both 3, in `task.rs` and here. Two constants that agree
/// today are one edit away from `task.must_escalate` saying a task is finished while
/// `after_review` sends it round again, and nothing would have failed to say so.
pub use crate::army::task::MAX_ATTEMPTS;

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
        "You are {}. {}\n\n{position}\n\n{}\n\n{rank}\n\n{STANDING}",
        agent.display,
        agent.remit,
        the_organisation()
    )
}

/// The whole chart, given to everybody.
///
/// Knowing who exists and being allowed to hand them work are different things, and until now
/// an agent was only told the second. Carl was asked how Miles was getting on and answered "I
/// do not know a Miles", which was true of what he had been told and useless to JJ: Miles is in
/// the compiled table, working for Olivia. An agent that cannot name the organisation cannot
/// point at the person who would know, so every question about somebody else's department dead
/// ends instead of being redirected.
///
/// The delegation rule is unchanged and is repeated here, because the risk of handing somebody
/// the whole chart is that they start reaching across it.
fn the_organisation() -> String {
    let mut lines = vec!["The whole organisation, so you can say who somebody is:".to_string()];
    for a in crate::army::org::everyone() {
        let under = match a.reports_to {
            Some(boss) => format!(" under {boss}"),
            None => String::new(),
        };
        lines.push(format!("  {} ({}{}): {}", a.name, a.rank, under, a.remit));
    }
    lines.push(
        "Knowing this is not permission to use it. You still hand work only to the agents \
         directly below you. If somebody asks about an agent who is not yours, say whose they \
         are and point at their lead rather than saying you have never heard of them."
            .to_string(),
    );
    lines.join("\n")
}

#[cfg(test)]
mod tests;
