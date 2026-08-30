//! Asking Hyprland what is on screen, and moving it, without handing out a shell.
//!
//! JJ moved to Hyprland on 2026 08 30 and wants Carl and the agents to help manage it. Hyprland
//! is driven entirely by `hyprctl`, so the obvious move is to put `Bash(hyprctl:*)` in the tool
//! lists and be done. That would be a mistake, and it is worth writing down why.
//!
//! **`hyprctl dispatch exec <anything>` runs an arbitrary command.** An agent holding raw
//! `hyprctl` does not have a window manager, it has a shell, and every tool list in this
//! codebase becomes decoration: Miles cannot run `Bash`, but Miles could run
//! `hyprctl dispatch exec rm -rf`. The allow list is the security design, and a tool that
//! tunnels past it is worse than a tool that was never granted.
//!
//! So agents get this instead. Reads are free, because knowing what is on screen cannot break
//! anything. Writes are a named list of dispatchers that only ever move a window or change a
//! workspace, and everything else is refused by name rather than by pattern. `exec`, `exit`,
//! `killactive` and `keyword` are all refused: the first is a shell, the second ends JJ's
//! session, the third throws away unsaved work in somebody else's window, and the fourth
//! rewrites the compositor's configuration at runtime.
//!
//! Refusing by allow list rather than by deny list on purpose. A deny list is wrong the day
//! Hyprland adds a dispatcher, and it adds them often.

use std::process::Command;

use serde::Deserialize;

use crate::{Error, Result};

mod dispatch;
mod read;

pub use dispatch::{ALLOWED, dispatch};
pub use read::{active_window, clients, monitors, only_match, workspaces};

/// One window Hyprland is showing.
#[derive(Debug, Clone, Deserialize)]
pub struct Client {
    pub address: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub workspace: Workspace,
    #[serde(default)]
    pub floating: bool,
    #[serde(default)]
    pub fullscreen: i64,
    #[serde(default)]
    pub pid: i64,
}

impl Client {
    /// What to call this window when talking to a person.
    ///
    /// The class is empty for some windows, the Command Panel included, so falling back to the
    /// title matters rather than being tidy: an agent told to focus a window with no name
    /// cannot do it.
    pub fn name(&self) -> &str {
        match self.class.is_empty() {
            false => &self.class,
            true => &self.title,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Workspace {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Monitor {
    pub name: String,
    #[serde(default)]
    pub width: i64,
    #[serde(default)]
    pub height: i64,
    #[serde(default)]
    pub scale: f64,
    #[serde(default)]
    pub focused: bool,
    #[serde(rename = "activeWorkspace", default)]
    pub active_workspace: Workspace,
}

/// Whether Hyprland is the compositor running right now.
///
/// Checked rather than assumed, because every command here is meaningless on anything else and
/// "hyprctl: command not found" is a worse answer than saying so.
pub fn running() -> bool {
    std::env::var("HYPRLAND_INSTANCE_SIGNATURE").is_ok_and(|s| !s.is_empty())
}

/// Runs one `hyprctl` call and hands back its stdout.
///
/// Every call goes through here so there is exactly one place that spawns the binary, and so
/// the not running case reads the same whichever command was asked for.
pub(crate) fn hyprctl(args: &[&str]) -> Result<String> {
    if !running() {
        return Err(Error::Refused(
            "Hyprland is not running, so there is nothing to ask. HYPRLAND_INSTANCE_SIGNATURE is \
             unset, which also happens inside a service that started before the session \
             environment was imported"
                .into(),
        ));
    }

    let out = Command::new("hyprctl").args(args).output().map_err(|e| {
        Error::Refused(format!(
            "cannot run hyprctl ({e}). It ships with Hyprland, so a missing one means this is \
             not a Hyprland session"
        ))
    })?;

    if !out.status.success() {
        return Err(Error::Refused(format!(
            "hyprctl {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests;
