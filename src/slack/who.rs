//! Turning a Slack user id into the name Carl should call somebody.
//!
//! Slack sends `U0BNSU5N96X`, never a name. Carl answering "hello U0BNSU5N96X" would be worse
//! than answering nobody, so the id has to be looked up, and looked up once rather than on
//! every message: a name does not change between two sentences and each lookup is a round
//! trip in the way of a reply.
//!
//! The rename table exists because the name on an account is not always the name a person
//! goes by. JJ's Slack account says "JJ_tmc Multiversal" and he is called JJ.

use std::collections::HashMap;

use super::Api;

/// Accounts whose display name is not what the person is actually called.
///
/// Matched case insensitively against the full name Slack gives back. A table rather than a
/// branch, so adding somebody is one line and needs no thought about where it goes.
const CALLED: [(&str, &str); 1] = [("jj_tmc multiversal", "JJ")];

/// Names, looked up once each.
pub struct Directory {
    known: HashMap<String, String>,
    /// Bot id to the user id behind it, looked up once each and remembered even when there
    /// is none, so a bot without an associated user is not asked about on every message.
    bots: HashMap<String, Option<String>>,
}

impl Default for Directory {
    fn default() -> Self {
        Self::new()
    }
}

impl Directory {
    pub fn new() -> Self {
        Self {
            known: HashMap::new(),
            bots: HashMap::new(),
        }
    }

    /// Who a message is from, as a mention target and a name.
    ///
    /// A bot message often carries a bot id and no user field. A `B` prefixed bot id is
    /// neither a lookup key for `users.info` nor something Slack renders as a mention, so
    /// using one as either fails quietly: the name lookup answers `user_not_found`, and the
    /// reply goes out as literal text reading `<@B0ALEX>` that notifies nobody. Which means
    /// an agent is never told about the answer the protocol says is addressed to it.
    ///
    /// `bots.info` is the only thing that maps a bot id to a user id, and even it may not have
    /// one, so the mention target is an `Option`. `None` means send no mention at all rather
    /// than one Slack cannot resolve, because a message that looks addressed and is not is
    /// worse than one that plainly is not. See bug 21.
    pub fn sender(&mut self, api: &Api, ask: &super::Ask) -> (Option<String>, String) {
        if !ask.user.is_empty() {
            let name = self.name_of(api, &ask.user);
            return (Some(ask.user.clone()), name);
        }

        let Some(bot_id) = ask.bot.as_deref() else {
            return (None, "somebody".to_string());
        };

        if let Some(known) = self.bots.get(bot_id).cloned() {
            let name = match &known {
                Some(u) => self.name_of(api, u),
                None => bot_id.to_string(),
            };
            return (known, name);
        }

        let (user, bot_name) = match api.bot_identity(bot_id) {
            Ok(found) => found,
            Err(e) => {
                eprintln!("  could not look up the bot {bot_id}: {e}");
                (None, None)
            }
        };
        self.bots.insert(bot_id.to_string(), user.clone());

        // The bot's own name is a better fallback than the raw id, and both are better than
        // inventing one, for the same reason `name_of` says.
        let name = match (&user, bot_name) {
            (Some(u), _) => self.name_of(api, u),
            (None, Some(n)) => n,
            (None, None) => bot_id.to_string(),
        };
        (user, name)
    }

    /// What to call the person behind this user id.
    ///
    /// Falls back to the id itself if Slack will not say, which is ugly and honest. Inventing
    /// a name would be worse, because Carl would then use it out loud to the person it is
    /// wrong about.
    pub fn name_of(&mut self, api: &Api, user_id: &str) -> String {
        if let Some(known) = self.known.get(user_id) {
            return known.clone();
        }

        let name = match api.user_name(user_id) {
            Ok(raw) => preferred(&raw),
            Err(e) => {
                eprintln!("  could not look up {user_id}: {e}");
                user_id.to_string()
            }
        };
        self.known.insert(user_id.to_string(), name.clone());
        name
    }
}

/// The name somebody actually goes by, given the name on their account.
pub fn preferred(raw: &str) -> String {
    let lower = raw.trim().to_lowercase();
    for (account, called) in CALLED {
        if lower == account {
            return called.to_string();
        }
    }
    raw.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one JJ asked for. His account name is not what he is called.
    #[test]
    fn jj_is_called_jj() {
        assert_eq!(preferred("JJ_tmc Multiversal"), "JJ");
        assert_eq!(preferred("jj_tmc multiversal"), "JJ");
        assert_eq!(preferred("  JJ_TMC MULTIVERSAL  "), "JJ");
    }

    /// Everybody else keeps their own name, which is the other half of the instruction.
    #[test]
    fn everyone_else_keeps_their_name() {
        assert_eq!(preferred("Hunter Zhang"), "Hunter Zhang");
        assert_eq!(preferred("Alex"), "Alex");
        assert_eq!(preferred("jj"), "jj", "a different account is not JJ's");
    }

    /// A partial match must not rename somebody. "JJ_tmc Multiversal Two" is a different
    /// person and calling them JJ would be worse than using the id.
    #[test]
    fn a_near_miss_is_not_a_match() {
        assert_eq!(
            preferred("JJ_tmc Multiversal Two"),
            "JJ_tmc Multiversal Two"
        );
        assert_eq!(
            preferred("Not JJ_tmc Multiversal"),
            "Not JJ_tmc Multiversal"
        );
    }
}
