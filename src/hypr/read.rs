//! The read side. Nothing here changes anything, so nothing here is gated.

use super::{Client, Monitor, Workspace, hyprctl};
use crate::{Error, Result};

/// Every window Hyprland is showing.
pub fn clients() -> Result<Vec<Client>> {
    parse(&hyprctl(&["-j", "clients"])?, "clients")
}

/// The window with focus, or `None` when focus is on empty desktop.
///
/// Hyprland answers an empty JSON object rather than null for no focus, which parses into a
/// client whose every field is a default and whose address is empty. That is the case worth
/// distinguishing: an agent told to describe the active window should say there is not one
/// rather than describe a window with no name.
pub fn active_window() -> Result<Option<Client>> {
    let text = hyprctl(&["-j", "activewindow"])?;
    let one: Option<Client> = serde_json::from_str(&text).unwrap_or(None);
    Ok(one.filter(|c| !c.address.is_empty()))
}

pub fn workspaces() -> Result<Vec<Workspace>> {
    parse(&hyprctl(&["-j", "workspaces"])?, "workspaces")
}

pub fn monitors() -> Result<Vec<Monitor>> {
    parse(&hyprctl(&["-j", "monitors"])?, "monitors")
}

/// One place that turns hyprctl's JSON into ours, so a version of Hyprland that changes a field
/// fails with the same sentence whichever command hit it first.
fn parse<T: serde::de::DeserializeOwned>(text: &str, what: &str) -> Result<Vec<T>> {
    serde_json::from_str(text).map_err(|e| {
        Error::Refused(format!(
            "could not read hyprctl {what} ({e}). This usually means Hyprland's JSON changed \
             shape, so the fix is here rather than on the machine"
        ))
    })
}

/// The one window whose class or title matches, refusing rather than guessing.
///
/// Refuses an ambiguous match by name. Two Chrome windows and an instruction to focus "chrome"
/// is a question, not a command, and picking the first one silently is how an agent moves the
/// wrong window and reports success.
pub fn only_match<'a>(windows: &'a [Client], wanted: &str) -> Result<&'a Client> {
    let needle = wanted.to_lowercase();
    let hits: Vec<&Client> = windows
        .iter()
        .filter(|c| {
            c.class.to_lowercase().contains(&needle) || c.title.to_lowercase().contains(&needle)
        })
        .collect();

    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(Error::Refused(format!(
            "no window matches {wanted}. `carl hypr windows` lists what is open"
        ))),
        n => Err(Error::Refused(format!(
            "{n} windows match {wanted}, so this would be a guess: {}. Say more of the title",
            hits.iter()
                .map(|c| format!("{} ({})", c.name(), c.title))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
