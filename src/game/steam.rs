//! Every Steam game, not just the one somebody thought to hardcode.
//!
//! Steam writes an `appmanifest_<id>.acf` per installed game, holding the name, the directory
//! it lives in, and when it was last played. That is the whole problem solved for anything
//! bought through Steam, without a table of names that goes stale the moment a new game is
//! installed.
//!
//! Detecting which one is running uses the full command line rather than the process name,
//! because `ps` truncates a process name to fifteen characters and plenty of games are called
//! something longer. The install path appears in the arguments of anything Steam launched, and
//! it is exact.

use std::path::{Path, PathBuf};

/// One installed game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    pub name: String,
    /// The folder under `steamapps/common`, which is what appears in a running command line.
    pub dir: String,
    /// Unix seconds, or zero when Steam has not recorded it.
    pub last_played: u64,
}

/// Things Steam installs that nobody plays.
///
/// Without this the most recently touched thing is nearly always a runtime, because they
/// update constantly and are launched alongside every actual game.
fn is_plumbing(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("runtime")
        || n.contains("proton")
        || n.contains("steamworks")
        || n.contains("redistributable")
        || n.starts_with("steam linux")
}

pub fn library() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    for guess in [
        ".steam/debian-installation/steamapps",
        ".steam/steam/steamapps",
        ".local/share/Steam/steamapps",
    ] {
        let dir = PathBuf::from(&home).join(guess);
        if dir.is_dir() {
            return Some(dir);
        }
    }
    None
}

/// Everything installed, most recently played first.
pub fn installed(steamapps: &Path) -> Vec<App> {
    let Ok(entries) = std::fs::read_dir(steamapps) else {
        return Vec::new();
    };

    let mut apps: Vec<App> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let n = e.file_name();
            let n = n.to_string_lossy();
            n.starts_with("appmanifest_") && n.ends_with(".acf")
        })
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| parse(&text))
        .filter(|a| !is_plumbing(&a.name))
        .collect();

    apps.sort_by_key(|a| std::cmp::Reverse(a.last_played));
    apps
}

/// Reads one manifest. The format is `"key"<tab>"value"`, one per line.
pub fn parse(text: &str) -> Option<App> {
    let field = |key: &str| -> Option<String> {
        text.lines()
            .map(str::trim)
            .find(|l| l.starts_with(&format!("\"{key}\"")))
            .and_then(|l| l.split('"').nth(3))
            .map(str::to_owned)
    };

    let name = field("name")?;
    if name.is_empty() {
        return None;
    }
    Some(App {
        dir: field("installdir").unwrap_or_else(|| name.clone()),
        last_played: field("LastPlayed")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        name,
    })
}

/// Which of these is running, judged by the command lines given.
///
/// Matched on the install path rather than the process name, because `ps` truncates a name to
/// fifteen characters and "The Planet Crafter" is longer than that. Anything Steam launched
/// carries its own directory in its arguments.
pub fn running<'a>(apps: &'a [App], command_lines: &str) -> Option<&'a App> {
    apps.iter()
        .find(|a| command_lines.contains(&format!("steamapps/common/{}/", a.dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANET: &str = r#"
"AppState"
{
	"appid"		"1284190"
	"name"		"The Planet Crafter"
	"installdir"		"The Planet Crafter"
	"LastPlayed"		"1786058144"
}"#;

    #[test]
    fn a_manifest_gives_the_name_and_when_it_was_played() {
        let a = parse(PLANET).unwrap();
        assert_eq!(a.name, "The Planet Crafter");
        assert_eq!(a.dir, "The Planet Crafter");
        assert_eq!(a.last_played, 1_786_058_144);
    }

    #[test]
    fn a_manifest_with_no_name_is_not_a_game() {
        assert_eq!(parse("\"AppState\"\n{\n}"), None);
        assert_eq!(parse(""), None);
    }

    /// Runtimes update constantly and launch alongside every real game, so without this the
    /// most recently touched thing is nearly always Proton.
    #[test]
    fn runtimes_and_proton_are_not_games() {
        for name in [
            "Steam Linux Runtime 2.0 (soldier)",
            "Proton Experimental",
            "Steamworks Common Redistributables",
        ] {
            assert!(is_plumbing(name), "{name} should not count as a game");
        }
        assert!(!is_plumbing("Factorio"));
        assert!(!is_plumbing("The Planet Crafter"));
    }

    /// `ps` truncates a process name to fifteen characters, and this game's name is longer,
    /// so matching on the name would never find it. The install path is exact.
    #[test]
    fn a_game_with_a_long_name_is_still_found_running() {
        let apps = vec![parse(PLANET).unwrap()];
        let args = "/home/jj/.steam/debian-installation/steamapps/common/The Planet Crafter/Planet Crafter.x86_64 --launcher";

        assert_eq!(
            running(&apps, args).map(|a| a.name.as_str()),
            Some("The Planet Crafter")
        );
        assert_eq!(running(&apps, "/usr/bin/chrome --type=renderer"), None);
    }

    /// One game's directory must not match another's because one name contains the other.
    #[test]
    fn a_similar_name_is_not_a_match() {
        let apps = vec![App {
            name: "Crafter".into(),
            dir: "Crafter".into(),
            last_played: 1,
        }];
        let args = "steamapps/common/The Planet Crafter/Planet Crafter.x86_64";
        assert_eq!(running(&apps, args), None, "Crafter is not Planet Crafter");
    }

    #[test]
    fn the_most_recently_played_comes_first() {
        let d = tempfile::tempdir().unwrap();
        for (id, name, played) in [
            ("1", "Old Game", "100"),
            ("2", "New Game", "900"),
            ("3", "Proton Experimental", "999"),
        ] {
            std::fs::write(
                d.path().join(format!("appmanifest_{id}.acf")),
                format!(
                    "\"AppState\"\n{{\n\t\"name\"\t\t\"{name}\"\n\t\"installdir\"\t\t\"{name}\"\n\t\"LastPlayed\"\t\t\"{played}\"\n}}"
                ),
            )
            .unwrap();
        }

        let apps = installed(d.path());
        assert_eq!(apps.len(), 2, "the runtime should be gone: {apps:?}");
        assert_eq!(apps[0].name, "New Game");
        assert_eq!(apps[1].name, "Old Game");
    }

    #[test]
    fn a_missing_library_is_empty_rather_than_an_error() {
        assert!(installed(Path::new("/definitely/not/here")).is_empty());
    }
}
