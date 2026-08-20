//! The three Slack web calls Carl makes.
//!
//! Slack answers every call with HTTP 200 and an `ok` field, so checking the status code
//! proves nothing. An expired token, a missing scope and a channel Carl was removed from all
//! arrive as a cheerful 200 with `"ok": false`, and treating that as success means posting
//! into the void and never being told.
//!
//! There are two ways to call, and using the wrong one fails in a way that points at the
//! wrong thing entirely. Methods that write take a JSON body. Methods that read take form
//! encoded parameters, and if you send them JSON the parameters are silently dropped rather
//! than refused. `users.info` with a JSON body reports `user_not_found`, which is true: no
//! user id arrived, so no user was found. It reads as a missing person and it is a missing
//! parameter. That cost an hour, so the two are separate functions here with names that say
//! which is which.

use serde_json::json;

use crate::{Error, Result};

const BASE: &str = "https://slack.com/api";

pub struct Api {
    bot: String,
    app: String,
}

impl Api {
    pub fn new(bot: impl Into<String>, app: impl Into<String>) -> Self {
        Self {
            bot: bot.into(),
            app: app.into(),
        }
    }

    /// Who Carl is, so he can recognise his own messages and stop replying to them.
    ///
    /// Called once at startup and treated as fatal if it fails. Guessing an id wrong means
    /// every reply Carl posts looks like a new question from someone else.
    pub fn whoami(&self) -> Result<Me> {
        let v = self.call("auth.test", &self.bot, json!({}))?;
        Ok(Me {
            user_id: field(&v, "user_id")?,
            // A bot message does not always carry a user field, so this is the other half of
            // recognising his own words coming back at him.
            bot_id: field(&v, "bot_id").unwrap_or_default(),
            team: field(&v, "team").unwrap_or_default(),
        })
    }

    /// Opens a Socket Mode connection and returns the websocket url to dial.
    ///
    /// The url is single use and expires in seconds, so it is fetched immediately before
    /// connecting and again on every reconnect.
    pub fn open_socket(&self) -> Result<String> {
        let v = self.call("apps.connections.open", &self.app, json!({}))?;
        field(&v, "url")
    }

    /// Says something in a channel without being asked first.
    ///
    /// A top level message rather than a thread reply, which is what starting a conversation
    /// means in Slack. Carl has to be in the channel already, so `/invite @Carl` first.
    /// Returns the new message timestamp, which is the id of the thread it starts.
    pub fn announce(&self, channel: &str, text: &str) -> Result<String> {
        let v = self.call(
            "chat.postMessage",
            &self.bot,
            json!({ "channel": channel, "text": text }),
        )?;
        field(&v, "ts")
    }

    /// The display name behind a user id.
    ///
    /// Form encoded, not JSON. See the note at the top of this file.
    ///
    /// Prefers what somebody chose to be called over what the account was registered with.
    /// display_name is the one people set deliberately, real_name is the fallback.
    pub fn user_name(&self, user_id: &str) -> Result<String> {
        let v = self.read("users.info", &self.bot, &[("user", user_id)])?;
        let p = v
            .get("user")
            .and_then(|u| u.get("profile"))
            .ok_or_else(|| Error::Refused("users.info gave no profile".into()))?;

        for key in ["display_name", "real_name"] {
            if let Some(n) = p.get(key).and_then(|n| n.as_str())
                && !n.trim().is_empty()
            {
                return Ok(n.trim().to_string());
            }
        }
        v.get("user")
            .and_then(|u| u.get("name"))
            .and_then(|n| n.as_str())
            .map(str::to_owned)
            .ok_or_else(|| Error::Refused("users.info gave no name at all".into()))
    }

    /// The user id behind a bot id, and the bot's name, from `bots.info`.
    ///
    /// A bot id is not a user id. It cannot be looked up with `users.info` and Slack will not
    /// render `<@B0ALEX>` as a mention, so a reply addressed that way never reaches the agent
    /// it was addressed to. `bots.info` is the only thing that maps one to the other, and even
    /// then the `user_id` field is optional, because not every bot has an associated user.
    /// See bug 21.
    pub fn bot_identity(&self, bot_id: &str) -> Result<(Option<String>, Option<String>)> {
        let v = self.read("bots.info", &self.bot, &[("bot", bot_id)])?;
        let b = v
            .get("bot")
            .ok_or_else(|| Error::Refused("bots.info gave no bot".into()))?;

        let user = b
            .get("user_id")
            .and_then(|u| u.as_str())
            .filter(|u| !u.trim().is_empty())
            .map(str::to_owned);
        let name = b
            .get("name")
            .and_then(|n| n.as_str())
            .filter(|n| !n.trim().is_empty())
            .map(str::to_owned);
        Ok((user, name))
    }

    /// Replaces a message already posted.
    ///
    /// How Carl shows an answer being written. Slack has no way to stream into a message, so
    /// the message is rewritten as the words arrive. Rate limited by Slack at roughly one a
    /// second per channel, which is why the caller paces itself rather than updating on every
    /// chunk.
    pub fn update(&self, channel: &str, ts: &str, text: &str) -> Result<()> {
        self.call(
            "chat.update",
            &self.bot,
            json!({ "channel": channel, "ts": ts, "text": text }),
        )?;
        Ok(())
    }

    /// Replies inside a thread, and gives back the timestamp so it can be rewritten later.
    pub fn post_returning(&self, channel: &str, thread_ts: &str, text: &str) -> Result<String> {
        let v = self.call(
            "chat.postMessage",
            &self.bot,
            json!({ "channel": channel, "thread_ts": thread_ts, "text": text }),
        )?;
        field(&v, "ts")
    }

    /// Replies inside a thread.
    pub fn post(&self, channel: &str, thread_ts: &str, text: &str) -> Result<()> {
        self.call(
            "chat.postMessage",
            &self.bot,
            json!({ "channel": channel, "thread_ts": thread_ts, "text": text }),
        )?;
        Ok(())
    }

    /// A method that changes something. These take a JSON body.
    fn call(
        &self,
        method: &str,
        token: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let res = ureq::post(format!("{BASE}/{method}"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .send_json(&body)
            .map_err(|e| Error::Refused(format!("slack {method} failed: {e}")))?;
        Self::unwrap(method, res)
    }

    /// A method that only looks something up. These take form encoded parameters.
    ///
    /// Sending JSON to one of these does not fail. The parameters are dropped and the method
    /// runs as though you had passed none, so the error you get back describes the empty
    /// request rather than your mistake.
    fn read(
        &self,
        method: &str,
        token: &str,
        params: &[(&str, &str)],
    ) -> Result<serde_json::Value> {
        let res = ureq::post(format!("{BASE}/{method}"))
            .header("Authorization", &format!("Bearer {token}"))
            .send_form(params.iter().copied())
            .map_err(|e| Error::Refused(format!("slack {method} failed: {e}")))?;
        Self::unwrap(method, res)
    }

    fn unwrap(
        method: &str,
        mut res: ureq::http::Response<ureq::Body>,
    ) -> Result<serde_json::Value> {
        let v: serde_json::Value = res
            .body_mut()
            .read_json()
            .map_err(|e| Error::Refused(format!("slack {method} gave no json: {e}")))?;

        // The status code was 200 either way. This is the only thing that says it worked.
        if v.get("ok").and_then(|o| o.as_bool()) != Some(true) {
            let why = v
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("no reason given");
            return Err(Error::Refused(format!(
                "slack {method} said no: {why}{}",
                hint(why)
            )));
        }
        Ok(v)
    }
}

#[derive(Debug, Clone)]
pub struct Me {
    pub user_id: String,
    pub bot_id: String,
    pub team: String,
}

fn field(v: &serde_json::Value, name: &str) -> Result<String> {
    v.get(name)
        .and_then(|f| f.as_str())
        .map(str::to_owned)
        .ok_or_else(|| Error::Refused(format!("slack answered without a {name} field")))
}

/// Slack's error strings are short and the fix is never obvious from them.
pub fn hint(error: &str) -> &'static str {
    match error {
        "invalid_auth" | "not_authed" | "token_revoked" => {
            ". Check the tokens in ~/.carl/slack.json. The bot token starts xoxb and the app \
             token starts xapp, and swapping them gives exactly this."
        }
        "missing_scope" => {
            ". Add the scope in the app's OAuth page, then reinstall the app to the \
             workspace. A new scope does nothing until it is reinstalled."
        }
        "message_not_found" | "cant_update_message" => {
            ". The message being rewritten is gone or belongs to somebody else."
        }
        "ratelimited" => {
            ". Too many updates too fast. Slack allows roughly one message edit a second per \
             channel."
        }
        "user_not_found" | "users_not_found" => {
            ". Either the id is not in this workspace, or the call sent a JSON body to a \
             method that only reads form encoded parameters, which drops the id silently. \
             See the note at the top of api.rs."
        }
        "not_in_channel" | "channel_not_found" => {
            ". Invite Carl to the channel with /invite @Carl."
        }
        "account_inactive" => ". The app was uninstalled from the workspace.",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two tokens look alike and are easy to swap, and the error Slack gives back says
    /// nothing about which one is wrong.
    #[test]
    fn the_confusing_errors_come_with_the_fix() {
        assert!(hint("invalid_auth").contains("xoxb"));
        assert!(hint("missing_scope").contains("reinstall"));
        assert!(hint("not_in_channel").contains("/invite"));
        assert_eq!(hint("something_new"), "");
    }

    /// The one that pointed at the wrong thing for an hour. Slack said the user did not
    /// exist, users.list showed that it did, and the real fault was sending JSON to a method
    /// that only reads form parameters, which drops them without complaining.
    #[test]
    fn user_not_found_mentions_the_encoding_trap() {
        let h = hint("user_not_found");
        assert!(h.contains("form encoded"), "{h}");
        assert!(h.contains("JSON"), "{h}");
    }
}
