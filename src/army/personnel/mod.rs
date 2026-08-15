//! What each agent keeps between one run and the next, on disk.
//!
//! `army::org` says who exists and who answers to whom, and it is compiled in because the
//! shape of the organisation is part of the program. `army::event` says what happened. This is
//! the third thing: the per agent folder that survives the process, holding the little that is
//! genuinely local to an agent and genuinely changes.
//!
//! ```text
//!   <home>/run/events.jsonl        the journal, appended only through army::event
//!   <home>/army/carl/              profile.json  config.json  state.json  README.md
//!   <home>/army/adrian/  mason/  nora/
//! ```
//!
//! **The property worth reading before the rest.** Rank, reporting line and permission are not
//! written down anywhere in this layer. They come from the compiled table, so there is no file
//! an agent could edit to promote itself. That is a stronger claim than "the check catches it",
//! because there is no check to get past and nothing to catch.
//!
//! What is left on disk is a department, a list of things the agent does not do, which model it
//! runs on, and which task it is holding. None of it answers a question about authority. A
//! state file that arrives carrying a field of that shape does not load at all, rather than
//! loading with the extra ignored, because a quietly dropped field is how somebody believes
//! they changed something and did not.
//!
//! Not built here, on purpose: layered memory, retrieval and promotion between memory levels,
//! onboarding handover, the global queue, the scheduler, reporting cadence, and the appeals
//! system. Enlisting records that an agent joined and where it sits, which is the foundation
//! those attach to rather than a small version of any of them.

mod config;
mod enlist;
mod files;
mod founding;
mod profile;
mod state;
mod store;

#[cfg(test)]
mod tests;

pub use config::{Config, DEFAULT_DEADLINE, Model};
pub use enlist::{announcement, enlist, found, may_enlist};
pub use profile::Profile;
pub use state::{NOTE_LIMIT, RECENT_KEPT, State};
pub use store::{Folder, Personnel};

use crate::army::org;

/// One agent's folder, in English, for whoever opens it.
///
/// Generated whenever the agent is saved and never parsed. The authority section is worked out
/// from `org` at the moment it is written rather than copied from anywhere, so it cannot come
/// to say something the code would disagree with, and editing it grants nothing.
pub fn describe(folder: &Folder) -> String {
    let agent = folder.agent;
    let below = org::reports_of(agent.name);
    let below = if below.is_empty() {
        "nobody, so this agent starts no work of its own".to_string()
    } else {
        below.iter().map(|a| a.name).collect::<Vec<_>>().join(", ")
    };
    let above = agent.reports_to.unwrap_or("nobody, this is the root");

    let mut out = format!(
        "# {}\n\n_Written by the harness whenever this agent is saved. Editing this file \
         changes nothing: rank, reporting line and permission all come from the compiled \
         organisation table, not from any file here._\n\n\
         ## Identity\n\n\
         - Name: {}\n- Role: {}\n- Rank: {}\n- Sits in: {}\n\n\
         ## Remit\n\n{}\n\n\
         ## Does not\n\n",
        agent.name,
        agent.name,
        agent.display,
        agent.rank,
        folder.profile.place(),
        agent.remit,
    );

    if folder.profile.does_not.is_empty() {
        out.push_str("Nothing recorded.\n");
    }
    for line in &folder.profile.does_not {
        out.push_str(&format!("- {line}\n"));
    }

    out.push_str(&format!(
        "\n## Authority\n\n\
         - Reports to: {}\n\
         - May hand work to: {}\n\
         - May change files: {}\n\
         - May enlist: {}\n\n\
         Work only ever moves one step, straight down. There is nowhere in this folder to \
         record a wider permission than that, which is deliberate.\n\n\
         ## Configuration\n\n- Model: {}\n- Deadline: {} seconds\n",
        above,
        below,
        if agent.rank.may_implement(false) {
            "yes"
        } else {
            "no, this rank states outcomes and judges them"
        },
        if may_enlist(agent.name).is_ok() { "yes, subject to JJ" } else { "no" },
        folder.config.model,
        folder.config.deadline_secs,
    ));

    for rule in &folder.config.rules {
        out.push_str(&format!("- Rule: {rule}\n"));
    }

    out.push_str(
        "\n## State\n\nWhich task this agent is holding is in `state.json`, the only file here \
         that changes often. The status of that task belongs to the task itself and is not \
         copied here.\n",
    );
    out
}
