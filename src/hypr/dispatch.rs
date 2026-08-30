//! The write side, which is an allow list and nothing else.
//!
//! See the note at the top of `hypr/mod.rs` for why this is not `Bash(hyprctl:*)`. The short
//! version is that `hyprctl dispatch exec` is a shell, and an agent holding a shell it was
//! never granted makes every tool list in this codebase decoration.

use super::hyprctl;
use crate::{Error, Result};

/// Every dispatcher an agent may use, and what each one does.
///
/// An allow list rather than a deny list, because a deny list is wrong the first time Hyprland
/// adds a dispatcher and it adds them often. Everything here moves a window or changes which
/// workspace is showing. Nothing here closes a window, runs a program, ends the session or
/// writes configuration.
///
/// Deliberately absent, and each for its own reason:
///
/// - `exec` runs an arbitrary command. It is a shell wearing a window manager's clothes.
/// - `exit` ends JJ's whole session, every unsaved thing in it included.
/// - `killactive` and `closewindow` throw away work in a window the agent does not own.
/// - `keyword` rewrites the compositor's configuration while it runs.
/// - `dpms` blanks the screen, which looks exactly like the machine dying.
/// - `movecursor` moves the pointer under a person's hand.
pub const ALLOWED: &[(&str, &str)] = &[
    ("workspace", "show a workspace"),
    ("focuswindow", "give a window focus"),
    ("movetoworkspace", "move a window to a workspace"),
    (
        "movetoworkspacesilent",
        "move a window without following it",
    ),
    ("togglefloating", "float or tile a window"),
    ("fullscreen", "toggle fullscreen on the focused window"),
    ("centerwindow", "centre a floating window"),
    ("pin", "keep a floating window on every workspace"),
    ("movewindow", "move the focused window in a direction"),
    ("swapwindow", "swap the focused window with its neighbour"),
    ("resizeactive", "resize the focused window"),
    ("cyclenext", "move focus to the next window"),
    ("splitratio", "change how the split divides"),
];

/// Runs one dispatcher, or refuses and says what is allowed.
///
/// The argument is passed through unsplit because Hyprland's dispatchers take their own
/// argument grammar (`movetoworkspace 3`, `resizeactive 100 0`). It never reaches a shell:
/// `Command` takes an argument vector, so there is nothing for a quote or a semicolon in the
/// argument to escape into.
pub fn dispatch(what: &str, arg: &str) -> Result<String> {
    let known = ALLOWED.iter().any(|(name, _)| *name == what);
    if !known {
        return Err(Error::Refused(format!(
            "{what} is not a dispatcher an agent may use. Allowed: {}.\n\nRefused on purpose: \
             exec runs any command, exit ends the session, killactive throws away somebody's \
             unsaved work, keyword rewrites the config while it runs. If JJ wants one of those, \
             he runs it himself.",
            ALLOWED
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let out = match arg.trim().is_empty() {
        true => hyprctl(&["dispatch", what])?,
        false => hyprctl(&["dispatch", what, arg.trim()])?,
    };

    // Hyprland answers "ok" on success and prose on failure, and it exits zero either way, so
    // the exit status is not the signal. Saying what it actually said is better than inventing
    // a success the caller cannot check.
    let said = out.trim();
    match said.eq_ignore_ascii_case("ok") {
        true => Ok(format!("{what} {arg}").trim().to_string()),
        false => Err(Error::Refused(format!(
            "Hyprland refused {what} {arg}: {said}"
        ))),
    }
}
