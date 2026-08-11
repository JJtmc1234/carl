//! What can be known about a Factorio game without looking at the screen.
//!
//! A screenshot says what is on screen this second. It does not say which expansion is
//! installed, which mods are loaded, or what the save is called, and those change the answer
//! more than anything visible does.
//!
//! Carl was giving vanilla advice to somebody playing Space Age with Bob's mods, where half
//! the tech tree is different and some of it does not exist. He had no way to know, and none
//! of it needed a screenshot: it is all sitting in `~/.factorio`.

use std::path::{Path, PathBuf};

/// What Carl can tell about the installation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Facts {
    /// Version string as the game reports it, like `2.0.77`.
    pub version: Option<String>,
    /// Expansions and flavours the build reports, like `space-age`.
    pub expansions: Vec<String>,
    /// Mod names, tidied, without version numbers.
    pub mods: Vec<String>,
    /// Save names, most recently written first.
    pub saves: Vec<String>,
}

pub fn home() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".factorio");
    dir.is_dir().then_some(dir)
}

/// Whether the game was last played within this long.
///
/// The log is rewritten every launch, so its age is the age of the last session. Reading a
/// timestamp beats parsing the file, and beats asking the window manager, which on Wayland
/// will not say.
pub fn played_within(dir: &Path, within: std::time::Duration) -> bool {
    let Ok(meta) = std::fs::metadata(dir.join("factorio-current.log")) else {
        return false;
    };
    meta.modified()
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age < within)
}

/// Reads everything cheap and non obvious about the installation.
pub fn facts(dir: &Path) -> Facts {
    let mut facts = Facts::default();

    if let Ok(log) = std::fs::read_to_string(dir.join("factorio-current.log")) {
        // Only the first line matters and the file runs to thousands. Reading it all is still
        // cheaper than being wrong about which expansion is installed.
        if let Some(first) = log.lines().next() {
            let (version, expansions) = from_banner(first);
            facts.version = version;
            facts.expansions = expansions;
        }
    }

    facts.mods = enabled_mods(&dir.join("mods"));
    facts.saves = saves_in(&dir.join("saves"));
    facts
}

/// Pulls the version and the flavours out of the log's first line.
///
/// It looks like this, and has for years:
/// `   0.000 2026-08-10 08:18:11; Factorio 2.0.77 (build 84539, linux64, steam, space-age)`
pub fn from_banner(line: &str) -> (Option<String>, Vec<String>) {
    let Some(after) = line.split("Factorio ").nth(1) else {
        return (None, Vec::new());
    };

    let version = after
        .split_whitespace()
        .next()
        .filter(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(str::to_owned);

    // Inside the brackets are a build number, a platform, a distributor and then any
    // expansions. Only the last group is interesting, and it is the one that is not a number
    // and not a known platform word.
    let expansions = after
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(inside, _)| {
            inside
                .split(',')
                .map(str::trim)
                .filter(|p| {
                    !p.is_empty()
                        && !p.starts_with("build")
                        && !matches!(
                            *p,
                            "linux64" | "win64" | "mac" | "steam" | "standalone" | "headless"
                        )
                })
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();

    (version, expansions)
}

/// Overhaul mods, which are the only ones that change what advice is correct.
///
/// JJ has over eighty mods installed. Listing them all costs more context than the whole rest
/// of the brief and buries the four that matter under sixty that do not. Nobody needs to be
/// told about `squeak through`. Everybody needs to be told about Sea Block, because in Sea
/// Block there are no ores on the map at all and every answer about mining is wrong.
///
/// Matched on a prefix of the tidied name with spaces removed, because these families ship as
/// dozens of separate mods with a shared stem and the stem is spelled inconsistently.
/// `space-exploration` and `space exploration` are the same family, and the first version of
/// this matched neither, because it stripped spaces from the mod and not from the pattern.
const OVERHAULS: &[(&str, &str)] = &[
    (
        "seablock",
        "Sea Block, where the map has no ores and everything starts from water",
    ),
    ("spaceexploration", "Space Exploration"),
    ("angels", "Angel's"),
    ("bob", "Bob's"),
    ("krastorio", "Krastorio 2"),
    ("py", "Pyanodons"),
    ("industrialrevolution", "Industrial Revolution"),
    ("nullius", "Nullius"),
    ("exoticindustries", "Exotic Industries"),
];

/// The overhauls present, described, and how many other mods there are.
pub fn overhauls(mods: &[String]) -> (Vec<&'static str>, usize) {
    let mut found: Vec<&'static str> = Vec::new();

    for (stem, described) in OVERHAULS {
        debug_assert!(
            !stem.contains(' '),
            "a stem with a space in it can never match, since names have their spaces removed"
        );
        let hit = mods.iter().any(|m| {
            m.to_lowercase()
                .replace([' ', '-', '_'], "")
                .starts_with(*stem)
        });
        if hit && !found.contains(described) {
            found.push(described);
        }
    }
    (found, mods.len())
}

/// Mods that are actually switched on.
///
/// Not the contents of the directory, which is what the first version of this read and which
/// was badly wrong. A mod being present on disk says nothing about whether it is in the game.
/// JJ has eighty eight mods downloaded and four enabled, so reading the directory reported
/// Sea Block, Angel's, Bob's and Space Exploration for somebody playing vanilla Space Age.
///
/// That is worse than reporting nothing. Carl answered a smelting question with Angel's ore
/// processing, which does not exist in that save, and no vanilla answer would have been that
/// far off. `mod-list.json` is the file the game itself reads, and it is the only honest
/// source.
fn enabled_mods(dir: &Path) -> Vec<String> {
    let Ok(raw) = std::fs::read_to_string(dir.join("mod-list.json")) else {
        // No list means the game has never been launched with mods, which means none are on.
        // Falling back to the directory here is exactly the mistake being fixed.
        return Vec::new();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };

    let mut out: Vec<String> = v
        .get("mods")
        .and_then(|m| m.as_array())
        .map(|list| {
            list.iter()
                .filter(|m| m.get("enabled").and_then(|e| e.as_bool()) == Some(true))
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                // `base` is always on and is not information.
                .filter(|n| *n != "base")
                .map(tidy_mod)
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out
}

/// `Bobs-updated-modpack_0.0.2` becomes `Bobs updated modpack`.
fn tidy_mod(stem: &str) -> String {
    let without_version = match stem.rsplit_once('_') {
        // Only if the tail actually looks like a version, since a name can contain one.
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit() || c == '.') => head,
        _ => stem,
    };
    without_version.replace(['-', '_'], " ").trim().to_string()
}

/// Save names, newest first, with autosaves left out.
///
/// Autosaves are the most recently written files almost always, so including them would bury
/// the save somebody actually named, which is the one that says where they are.
fn saves_in(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(std::time::SystemTime, String)> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let stem = name.strip_suffix(".zip")?.to_string();
            if stem.starts_with('_') {
                return None;
            }
            let at = e.metadata().ok()?.modified().ok()?;
            Some((at, stem))
        })
        .collect();

    // Newest first, so reversed. sort_by_key cannot borrow the key here, which is why this
    // is a comparator rather than the shorter form clippy prefers.
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    found.into_iter().map(|(_, n)| n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every stem is compared against a name with its spaces taken out, so a stem containing
    /// one can never match. That silently lost Space Exploration, which is installed here.
    #[test]
    fn no_overhaul_stem_can_be_impossible_to_match() {
        for (stem, name) in OVERHAULS {
            assert!(
                !stem.contains(' '),
                "{name} has an unmatchable stem: {stem:?}"
            );
            assert_eq!(*stem, stem.to_lowercase(), "{name} stem must be lowercase");
        }
    }

    /// The same family is spelled several ways across its own mods.
    #[test]
    fn a_family_is_found_however_it_is_spelled() {
        for spelling in [
            "space exploration",
            "space-exploration",
            "Space Exploration graphics",
            "spaceexploration postprocess",
        ] {
            let (found, _) = overhauls(&[spelling.to_string()]);
            assert_eq!(found, vec!["Space Exploration"], "missed {spelling:?}");
        }
    }

    /// The real line from JJ's machine. Getting the expansion wrong means advising him about
    /// a tech tree he does not have.
    #[test]
    fn the_banner_gives_the_version_and_the_expansion() {
        let (v, e) = from_banner(
            "   0.000 2026-08-10 08:18:11; Factorio 2.0.77 (build 84539, linux64, steam, space-age)",
        );
        assert_eq!(v.as_deref(), Some("2.0.77"));
        assert_eq!(e, vec!["space-age"]);
    }

    /// Vanilla has no expansion, and reporting one that is not there is worse than reporting
    /// none at all.
    #[test]
    fn a_vanilla_build_reports_no_expansion() {
        let (v, e) = from_banner("   0.000 Factorio 1.1.109 (build 61976, linux64, steam)");
        assert_eq!(v.as_deref(), Some("1.1.109"));
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn a_line_that_is_not_a_banner_gives_nothing() {
        for line in ["", "   0.000 Operating system: Linux", "Factorio is great"] {
            let (v, e) = from_banner(line);
            assert!(v.is_none(), "{line}");
            assert!(e.is_empty(), "{line}");
        }
    }

    /// The bug JJ caught. Eighty eight mods on disk, four switched on, and reading the
    /// directory reported four overhauls to somebody playing vanilla Space Age. The advice
    /// that came out was further from right than the vanilla advice it replaced.
    #[test]
    fn only_the_mods_that_are_switched_on_are_reported() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("mod-list.json"),
            r#"{"mods":[
                {"name":"base","enabled":true},
                {"name":"space-age","enabled":true},
                {"name":"quality","enabled":true},
                {"name":"bobores","enabled":false},
                {"name":"SeaBlockPack","enabled":false}
            ]}"#,
        )
        .unwrap();
        // On disk but switched off, which is the whole point.
        std::fs::write(d.path().join("bobores_1.0.0.zip"), "x").unwrap();

        let mods = enabled_mods(d.path());
        assert_eq!(mods, vec!["quality", "space age"], "{mods:?}");

        let (overhauls, _) = overhauls(&mods);
        assert!(
            overhauls.is_empty(),
            "vanilla must look vanilla: {overhauls:?}"
        );
    }

    /// No list at all means nothing is on. Falling back to the directory is the mistake.
    #[test]
    fn no_mod_list_means_no_mods() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bobores_1.0.0.zip"), "x").unwrap();
        assert!(enabled_mods(d.path()).is_empty());
    }

    /// A version number is noise to somebody being given advice.
    #[test]
    fn a_mod_name_loses_its_version() {
        assert_eq!(
            tidy_mod("Bobs-updated-modpack_0.0.2"),
            "Bobs updated modpack"
        );
        assert_eq!(tidy_mod("FactorySearch_1.14.3"), "FactorySearch");
        assert_eq!(tidy_mod("KS_Power_2.0.0"), "KS Power");
    }

    /// An underscore is not always a version separator, and cutting at the wrong one renames
    /// the mod.
    #[test]
    fn an_underscore_that_is_not_a_version_is_kept() {
        assert_eq!(tidy_mod("Helpfull_stuff"), "Helpfull stuff");
        assert_eq!(tidy_mod("some_mod_name"), "some mod name");
    }

    /// Autosaves are almost always the newest files, so leaving them in buries the save
    /// somebody actually named, which is the one that says where they are.
    #[test]
    fn autosaves_are_left_out_and_the_newest_is_first() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        for name in [
            "First Space Age.zip",
            "_autosave1.zip",
            "Space Age but the Vanilla Nauvis Part is Done.zip",
        ] {
            std::fs::write(dir.join(name), "x").unwrap();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let saves = saves_in(dir);
        assert!(!saves.iter().any(|s| s.starts_with('_')), "{saves:?}");
        assert_eq!(saves[0], "Space Age but the Vanilla Nauvis Part is Done");
        assert_eq!(saves.len(), 2);
    }

    /// A game played last week is noise, and it is how a mod list ends up in an answer about
    /// homework.
    #[test]
    fn a_game_from_last_week_does_not_count_as_being_played() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("factorio-current.log"), "x").unwrap();

        assert!(played_within(d.path(), std::time::Duration::from_secs(60)));
        assert!(
            !played_within(d.path(), std::time::Duration::ZERO),
            "nothing is newer than no time at all"
        );
    }

    #[test]
    fn no_log_means_it_was_never_played() {
        let d = tempfile::tempdir().unwrap();
        assert!(!played_within(
            d.path(),
            std::time::Duration::from_secs(999)
        ));
    }

    #[test]
    fn a_missing_directory_is_empty_rather_than_an_error() {
        assert!(saves_in(Path::new("/definitely/not/here")).is_empty());
        assert!(enabled_mods(Path::new("/definitely/not/here")).is_empty());
    }
}
