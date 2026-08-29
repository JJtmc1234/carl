//! What each founding agent's folder starts as.
//!
//! Only the part `army::org` does not already hold. The name, the rank, the reporting line and
//! the remit are all in the table and none of them is repeated here, so this is a short file on
//! purpose. It is a seed rather than a source of truth: once an agent has a folder, the folder
//! is what counts.

use super::config::Config;
use super::hours::Hours;
use super::profile::Profile;
use crate::army::Rank;

/// What hours an agent keeps when it is enlisted.
///
/// Normal agents are off overnight. An agent that never stops is an agent whose context and cost
/// grow without anybody choosing it, and nobody chooses a thing that has no moment of being
/// decided.
///
/// The chief is the exception, and it is the only one. Carl is what JJ talks to, and an assistant
/// that is off between eleven and seven is an assistant that is off exactly when somebody
/// remembers something at midnight. Everybody else can be started when there is work.
///
/// By rank rather than by name, so a second lead added to `org.rs` gets the ordinary arrangement
/// without anybody remembering to come back here.
pub fn founding_config(rank: Rank) -> Config {
    Config {
        hours: match rank {
            Rank::Chief => None,
            _ => Some(Hours::night()),
        },
        ..Config::default()
    }
}

/// What each founding agent's folder starts as.
///
/// Only the part `org` does not already hold. Everything else about these four is in the
/// table and is not repeated.
pub fn founding_profile(name: &str) -> Profile {
    match name {
        "carl" => Profile::new(
            None,
            None,
            &[
                "Writes, reviews or rewrites any of the work.",
                "Reaches past a department lead to the people under them.",
                "Decides anything JJ has already decided.",
            ],
        ),
        "adrian" => Profile::new(
            Some("engineering"),
            None,
            &[
                "Writes code.",
                "Decides how a sub department does its work.",
                "Passes a disagreement upward that he could settle himself.",
            ],
        ),
        "iris" => Profile::new(
            Some("engineering"),
            None,
            &[
                "Changes any code. She reports what is wrong and never repairs it.",
                "Writes an issue for a file she has not read.",
                "Raises something already covered by an open issue.",
                "States a bug she cannot name the mechanism of, rather than saying she is \
                 unsure.",
            ],
        ),
        "evan" => Profile::new(
            Some("engineering"),
            None,
            &[
                "Deletes or edits anything without asking JJ first.",
                "Calls a fix done without a test that fails without it.",
                "Fixes something in a different order from the one Adrian gave.",
                "Closes an issue he has not actually fixed.",
            ],
        ),
        "mason" => Profile::new(
            Some("factorio"),
            None,
            &[
                "Writes the implementation.",
                "Gives his worker two tasks at once.",
                "Accepts work he has not checked.",
            ],
        ),
        "nora" => Profile::new(
            Some("factorio"),
            None,
            &[
                "Gives herself or anybody else the next task.",
                "Reports to anybody except Mason.",
                "Calls something blocked before she has read the code and tried to debug it.",
            ],
        ),
        "olivia" => Profile::new(
            Some("operations"),
            None,
            &[
                "Writes the replies herself instead of reviewing them.",
                "Lets anything be sent or deleted on her own authority.",
                "Decides what matters to JJ without asking him.",
            ],
        ),
        "miles" => Profile::new(
            Some("operations"),
            None,
            &[
                // Sending was allowed on 2026 08 29. This line used to forbid it and
                // contradicted both the org remit and the tools he actually holds, which is
                // the worst kind of stale text: an agent reads it and hesitates over work it
                // was told to do. Trashing and archiving stay out, and they stay out in the
                // tool list rather than only here.
                "Deletes, archives or marks anything as spam. He has no tool for any of it.",
                "Sends outside the task he was given, or without the safety checks.",
                "Writes a draft full of dashes and semicolons. JJ is graded on that.",
                "Reports a message as important without saying why.",
                "Reads anything outside the inbox he was given.",
            ],
        ),
        "serena" => Profile::new(
            Some("security"),
            None,
            &[
                "Grants anybody a privilege. There is nowhere to write one down and there is \
                 not meant to be.",
                "Acts on a finding rather than reporting it.",
            ],
        ),
        "rowan" => Profile::new(
            Some("research"),
            None,
            &[
                "Reports what he read as what he concluded.",
                "Answers from memory where he could have gone and checked.",
            ],
        ),
        _ => Profile::default(),
    }
}
