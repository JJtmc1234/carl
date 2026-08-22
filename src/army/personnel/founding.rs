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
pub(super) fn founding_config(rank: Rank) -> Config {
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
pub(super) fn founding_profile(name: &str) -> Profile {
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
            Some("coding"),
            None,
            &[
                "Writes code.",
                "Decides how a sub department does its work.",
                "Passes a disagreement upward that he could settle himself.",
            ],
        ),
        "mason" => Profile::new(
            Some("coding"),
            Some("factorio"),
            &[
                "Writes the implementation.",
                "Gives his worker two tasks at once.",
                "Accepts work he has not checked.",
            ],
        ),
        "nora" => Profile::new(
            Some("coding"),
            Some("factorio"),
            &[
                "Gives herself or anybody else the next task.",
                "Reports to anybody except Mason.",
                "Calls something blocked before she has read the code and tried to debug it.",
            ],
        ),
        _ => Profile::default(),
    }
}
