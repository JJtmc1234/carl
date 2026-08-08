//! Where the two Slack tokens live, and refusing to run if they are readable by anyone else.
//!
//! Two tokens, because Socket Mode needs both and they do different jobs. The bot token
//! (`xoxb-`) acts as Carl inside the workspace. The app level token (`xapp-`) opens the
//! websocket. They look alike, they are easy to swap, and Slack's error for a swap says only
//! `invalid_auth`.
//!
//! A bot token can read and post as Carl in every channel he is in. That is not a password
//! but it is not far off, so the file has to be private and this checks rather than assumes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Tokens {
    /// Starts `xoxb-`. Acts as Carl in the workspace.
    pub bot: String,
    /// Starts `xapp-`. Opens the Socket Mode websocket.
    pub app: String,
}

impl Tokens {
    pub fn path(home: &Path) -> PathBuf {
        home.join("slack.json")
    }

    /// Reads the tokens, from the environment first and the file otherwise.
    ///
    /// The environment wins so a service unit can supply them without a file at all, which is
    /// what running under systemd wants.
    pub fn load(home: &Path) -> Result<Self> {
        if let (Ok(bot), Ok(app)) = (
            std::env::var("CARL_SLACK_BOT_TOKEN"),
            std::env::var("CARL_SLACK_APP_TOKEN"),
        ) {
            return Self { bot, app }.checked();
        }

        let path = Self::path(home);
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            Error::Refused(format!(
                "cannot read {}: {e}\n\nWrite it like this, then chmod 600 it:\n\
                 {{\n  \"bot\": \"xoxb-...\",\n  \"app\": \"xapp-...\"\n}}",
                path.display()
            ))
        })?;

        // Checked before parsing, so a world readable file is refused rather than used once
        // and complained about afterwards.
        let mode = std::fs::metadata(&path)?.permissions().mode() & 0o077;
        if mode != 0 {
            return Err(Error::Refused(format!(
                "{} is readable by other users. Run: chmod 600 {}",
                path.display(),
                path.display()
            )));
        }

        let tokens: Self = serde_json::from_str(&raw)
            .map_err(|e| Error::Refused(format!("{} is not valid json: {e}", path.display())))?;
        tokens.checked()
    }

    /// Catches the swap before it reaches Slack.
    ///
    /// Slack answers a swapped pair with `invalid_auth`, which says nothing about which token
    /// is wrong or that swapping is even a thing that happens. The prefixes make it obvious
    /// here and there is no reason to find out over the network.
    fn checked(self) -> Result<Self> {
        if !self.bot.starts_with("xoxb-") {
            return Err(Error::Refused(
                "the bot token should start with xoxb-. If it starts with xapp- the two are \
                 the wrong way round."
                    .into(),
            ));
        }
        if !self.app.starts_with("xapp-") {
            return Err(Error::Refused(
                "the app token should start with xapp-. If it starts with xoxb- the two are \
                 the wrong way round."
                    .into(),
            ));
        }
        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, body: &str, mode: u32) -> PathBuf {
        let p = Tokens::path(dir);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn a_private_file_with_both_tokens_loads() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"bot":"xoxb-abc","app":"xapp-def"}"#, 0o600);
        let t = Tokens::load(d.path()).unwrap();
        assert_eq!(t.bot, "xoxb-abc");
        assert_eq!(t.app, "xapp-def");
    }

    /// A bot token in a world readable file is a bot token anyone on the machine can post as.
    #[test]
    fn a_readable_file_is_refused_rather_than_used() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"bot":"xoxb-a","app":"xapp-b"}"#, 0o644);
        let err = Tokens::load(d.path()).unwrap_err().to_string();
        assert!(err.contains("chmod 600"), "{err}");
    }

    /// The mistake everyone makes, caught here rather than as invalid_auth from Slack.
    #[test]
    fn swapped_tokens_are_named_as_swapped() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"bot":"xapp-a","app":"xoxb-b"}"#, 0o600);
        let err = Tokens::load(d.path()).unwrap_err().to_string();
        assert!(err.contains("wrong way round"), "{err}");
    }

    #[test]
    fn a_missing_file_says_what_to_write_in_it() {
        let d = tempfile::tempdir().unwrap();
        let err = Tokens::load(d.path()).unwrap_err().to_string();
        assert!(err.contains("xoxb-"), "{err}");
        assert!(err.contains("chmod 600"), "{err}");
    }
}
