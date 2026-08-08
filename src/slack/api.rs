//! The three Slack web calls Carl makes.
//!
//! Slack answers every call with HTTP 200 and an `ok` field, so checking the status code
//! proves nothing. An expired token, a missing scope and a channel Carl was removed from all
//! arrive as a cheerful 200 with `"ok": false`, and treating that as success means posting
//! into the void and never being told.

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

    /// Replies inside a thread.
    pub fn post(&self, channel: &str, thread_ts: &str, text: &str) -> Result<()> {
        self.call(
            "chat.postMessage",
            &self.bot,
            json!({ "channel": channel, "thread_ts": thread_ts, "text": text }),
        )?;
        Ok(())
    }

    fn call(
        &self,
        method: &str,
        token: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let mut res = ureq::post(format!("{BASE}/{method}"))
            .header("Authorization", &format!("Bearer {token}"))
            .header("Content-Type", "application/json; charset=utf-8")
            .send_json(&body)
            .map_err(|e| Error::Refused(format!("slack {method} failed: {e}")))?;

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
}
