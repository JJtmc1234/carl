//! Noticing what JJ is playing, without being told.
//!
//! Every screenshot Carl took was a fresh stranger. He could see a factory and had no idea
//! which game it was, which version, or whether the tech tree in front of him was the one he
//! knows about. He gave vanilla Factorio advice to somebody playing Space Age with Bob's
//! mods, where half the answer does not exist.
//!
//! None of that needed a screenshot. It is sitting in the process list and in the game's own
//! files, and it is cheap enough to check on every turn.
//!
//! Window titles would be the obvious signal and are not available. This machine runs
//! Wayland, where a program cannot ask what any other window is called, and asking GNOME
//! Shell directly returns false. The process list is unaffected by any of that.

use std::process::Command;

pub mod factorio;
pub mod seen;
pub mod steam;

/// What Carl can say about the game, if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    pub name: String,
    /// True when it is running now, false when it is the last one played.
    pub running: bool,
    /// Anything specific worth knowing, one line each. Empty for a game Carl knows nothing
    /// about beyond its name, which is still worth saying.
    pub detail: Vec<String>,
}

/// How long after quitting a game still counts as being played.
///
/// Half the questions arrive in the ten minutes after closing the window, and a Carl who
/// forgets the instant it shuts is less use than one who says what he last saw. A day later
/// it is noise, and telling him about a game nobody is playing is how a mod list ends up in
/// an answer about homework.
const RECENT: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Looks for a game, running or played within the last few hours.
///
/// Steam first, because it covers everything bought through it without a table of names that
/// goes stale the moment something new is installed. Factorio is then asked for its own extra
/// detail, since expansions and mods change what a correct answer is and nothing generic can
/// know that.
///
/// A game in a browser tab cannot be found this way at all. Chrome's renderer processes do
/// not carry the page in their arguments, Wayland will not say what a window is called, and
/// reading the browser's own session files is somebody else's data. So a web game is named by
/// being told, and kept in the same picture as everything else.
pub fn playing() -> Option<Playing> {
    let args = command_lines();
    let apps = steam::library()
        .map(|d| steam::installed(&d))
        .unwrap_or_default();

    if let Some(app) = steam::running(&apps, &args) {
        return Some(described(&app.name, true));
    }
    if let Some(name) = other_running(&args) {
        return Some(described(name, true));
    }

    let recent = apps.first().filter(|a| within(a.last_played, RECENT))?;
    Some(described(&recent.name, false))
}

/// Games worth noticing that Steam does not know about.
///
/// Short on purpose. Anything bought through Steam is already covered, so this is only for
/// things installed another way.
fn other_running(args: &str) -> Option<&'static str> {
    for (needle, name) in [
        ("net.minecraft", "Minecraft"),
        ("PrismLauncher", "Minecraft"),
        ("minecraft-launcher", "Minecraft"),
    ] {
        if args.contains(needle) {
            return Some(name);
        }
    }
    None
}

/// Adds whatever is known about this particular game.
fn described(name: &str, running: bool) -> Playing {
    let mut detail = Vec::new();
    if name == "Factorio"
        && let Some(dir) = factorio::home()
    {
        detail = factorio::detail(&factorio::facts(&dir));
    }
    Playing {
        name: name.to_string(),
        running,
        detail,
    }
}

fn within(unix_seconds: u64, window: std::time::Duration) -> bool {
    if unix_seconds == 0 {
        return false;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|now| now.as_secs().saturating_sub(unix_seconds) < window.as_secs())
        .unwrap_or(false)
}

/// Full command lines, not process names, because `ps` truncates a name to fifteen characters
/// and plenty of games are called something longer.
fn command_lines() -> String {
    let Ok(out) = Command::new("ps").args(["-eo", "args="]).output() else {
        return String::new();
    };
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What Carl is told about the game.
///
/// Always something when a game was found, even if only the name. Knowing somebody is playing
/// The Planet Crafter is worth having, and saying so lets them correct him, which beats him
/// quietly guessing.
pub fn brief(found: &Playing) -> Option<String> {
    let state = if found.running {
        "running now"
    } else {
        "not running at the moment, this is the last game played"
    };
    let mut lines = vec![format!("{}, {state}.", found.name)];
    lines.extend(found.detail.iter().cloned());
    Some(format!("# The game\n\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game Carl knows nothing about beyond its name is still worth mentioning. Saying so
    /// lets somebody correct him, which beats him quietly guessing.
    #[test]
    fn a_game_with_no_detail_is_still_announced() {
        let b = brief(&Playing {
            name: "The Planet Crafter".into(),
            running: true,
            detail: Vec::new(),
        })
        .expect("the name alone is worth saying");
        assert!(b.contains("The Planet Crafter"), "{b}");
        assert!(b.contains("running now"), "{b}");
    }

    /// Half the questions arrive just after quitting, and saying so beats pretending the game
    /// is up or saying nothing.
    #[test]
    fn a_game_that_has_stopped_is_described_as_stopped() {
        let b = brief(&Playing {
            name: "Factorio".into(),
            running: false,
            detail: vec!["Expansions: space-age.".into()],
        })
        .unwrap();
        assert!(b.contains("last game played"), "{b}");
        assert!(b.contains("space-age"), "detail must survive");
    }

    /// Zero means Steam never recorded a play, which is not the same as playing in 1970.
    #[test]
    fn never_played_is_not_recently_played() {
        assert!(!within(0, RECENT));
        assert!(!within(1_000_000, RECENT));
    }

    #[test]
    fn something_played_a_moment_ago_is_recent() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(within(now - 60, RECENT));
    }

    /// Minecraft is not on Steam, so it needs its own rule, and the launcher is what shows up.
    #[test]
    fn a_game_outside_steam_is_still_found() {
        assert_eq!(
            other_running("/usr/lib/jvm/bin/java -cp x net.minecraft.client.main.Main"),
            Some("Minecraft")
        );
        assert_eq!(other_running("/usr/bin/chrome --type=renderer"), None);
    }
}
