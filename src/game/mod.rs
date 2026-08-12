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

/// A game Carl knows how to notice.
struct Known {
    /// What the process is called. Matched exactly, because a substring match on something
    /// short catches the wrong thing.
    process: &'static str,
    name: &'static str,
}

const KNOWN: &[Known] = &[
    Known {
        process: "factorio",
        name: "Factorio",
    },
    Known {
        process: "Minecraft",
        name: "Minecraft",
    },
    Known {
        process: "stardew",
        name: "Stardew Valley",
    },
    Known {
        process: "FactoryGame",
        name: "Satisfactory",
    },
];

/// What Carl can say about the game, if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playing {
    pub name: &'static str,
    /// True when the game is running now, false when this is the last one played.
    pub running: bool,
    pub facts: factorio::Facts,
}

/// How long after quitting a game still counts as the game being played.
///
/// Half the questions arrive in the ten minutes after closing the window, and a Carl who
/// forgets the instant it shuts is less use than one who says what he last saw. A day later
/// it is noise, and telling him about a game nobody is playing is how a mod list ends up in
/// an answer about homework.
const RECENT: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

/// Looks for a game, running or played within the last few hours.
pub fn playing() -> Option<Playing> {
    let processes = running_processes();
    let found = KNOWN
        .iter()
        .find(|k| processes.iter().any(|p| p == k.process));

    match found {
        Some(k) if k.process == "factorio" => Some(Playing {
            name: k.name,
            running: true,
            facts: factorio::home()
                .map(|d| factorio::facts(&d))
                .unwrap_or_default(),
        }),
        Some(k) => Some(Playing {
            name: k.name,
            running: true,
            facts: factorio::Facts::default(),
        }),
        // Nothing running. Factorio leaves enough behind to be worth mentioning, but only
        // while it is still recent.
        None => {
            let dir = factorio::home()?;
            factorio::played_within(&dir, RECENT).then(|| Playing {
                name: "Factorio",
                running: false,
                facts: factorio::facts(&dir),
            })
        }
    }
}

fn running_processes() -> Vec<String> {
    let Ok(out) = Command::new("ps").args(["-eo", "comm="]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

/// What Carl is told about the game, if anything is worth telling him.
///
/// Returns `None` rather than an empty section. A heading with nothing under it invites a
/// model to fill the gap, and inventing a mod list is worse than having none.
pub fn brief(found: &Playing) -> Option<String> {
    let f = &found.facts;
    let mut lines = Vec::new();

    let state = if found.running {
        "running now"
    } else {
        "not running at the moment, this is the last game played"
    };
    match &f.version {
        Some(v) => lines.push(format!("{} {v}, {state}.", found.name)),
        None => lines.push(format!("{}, {state}.", found.name)),
    }

    if !f.expansions.is_empty() {
        lines.push(format!(
            "Expansions: {}. The tech tree is not the vanilla one, so do not answer as though \
             it were.",
            f.expansions.join(", ")
        ));
    }
    if !f.mods.is_empty() {
        let (overhauls, total) = factorio::overhauls(&f.mods);
        if overhauls.is_empty() {
            lines.push(format!(
                "{total} mods enabled: {}. None of them are overhauls, so the recipes and the \
                 tech tree are the ones the game ships with.",
                f.mods.join(", ")
            ));
        } else {
            lines.push(format!(
                "{total} mods enabled, including {}. This is not the base game. Recipes, costs \
                 and the whole tech tree are different, and a lot of standard advice is simply \
                 wrong here. Say when you are not sure whether something applies.",
                overhauls.join(", plus ")
            ));
        }
    }
    if let Some(save) = f.saves.first() {
        lines.push(format!(
            "Most recent save: \"{save}\". The name is often the best clue about how far in \
             they are."
        ));
    }

    // One line is only the name, which the question would have said anyway.
    if lines.len() < 2 {
        return None;
    }
    Some(format!("# The game\n\n{}", lines.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> factorio::Facts {
        factorio::Facts {
            version: Some("2.0.77".into()),
            expansions: vec!["space-age".into()],
            mods: vec![
                "bobplates".into(),
                "angelsrefining".into(),
                "SeaBlockPack".into(),
                "squeak through 2".into(),
            ],
            saves: vec!["Space Age but the Vanilla Nauvis Part is Done".into()],
        }
    }

    /// The whole reason this exists. Carl advised somebody on vanilla while they played Space
    /// Age with Bob's mods, and neither fact was visible in a screenshot.
    #[test]
    fn the_brief_says_it_is_not_vanilla() {
        let b = brief(&Playing {
            name: "Factorio",
            running: true,
            facts: facts(),
        })
        .expect("there is plenty to say");

        assert!(b.contains("2.0.77"), "{b}");
        assert!(b.contains("space-age"), "{b}");
        assert!(b.contains("not the vanilla one"), "{b}");
        assert!(b.contains("Bob's"), "{b}");
        assert!(b.contains("Sea Block"), "the one that changes everything");
        assert!(!b.contains("squeak"), "quality of life mods are noise");
        assert!(b.contains("Nauvis"), "the save name is a real clue");
    }

    /// A heading with nothing under it invites a model to fill the gap, and an invented mod
    /// list is worse than no mod list.
    #[test]
    fn nothing_worth_saying_produces_no_section() {
        assert_eq!(
            brief(&Playing {
                name: "Minecraft",
                running: true,
                facts: factorio::Facts::default(),
            }),
            None
        );
    }

    /// Half the questions arrive just after quitting, and saying so is better than pretending
    /// the game is up or saying nothing at all.
    #[test]
    fn a_game_that_has_stopped_is_described_as_stopped() {
        let b = brief(&Playing {
            name: "Factorio",
            running: false,
            facts: facts(),
        })
        .unwrap();
        assert!(b.contains("last game played"), "{b}");
    }

    /// Over eighty mods installed, and four of them decide whether an answer is right. The
    /// rest are noise that would cost more context than the whole rest of the brief.
    #[test]
    fn only_the_overhauls_are_named() {
        let many: Vec<String> = (0..80).map(|i| format!("some mod {i}")).collect();
        let (found, total) = factorio::overhauls(&many);
        assert!(found.is_empty());
        assert_eq!(total, 80);

        let mut with = many.clone();
        with.push("bobores".into());
        let (found, total) = factorio::overhauls(&with);
        assert_eq!(found, vec!["Bob's"]);
        assert_eq!(total, 81, "the count still says how many there really are");
    }

    /// Sea Block has no ores on the map at all, so every answer about mining is wrong unless
    /// Carl knows. It is the clearest case for naming the overhaul rather than the mods.
    #[test]
    fn sea_block_says_what_makes_it_different() {
        let (found, _) = factorio::overhauls(&["SeaBlockPack".to_string()]);
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("no ores"), "{found:?}");
    }

    /// A vanilla install must not be told it has expansions it does not have.
    #[test]
    fn vanilla_is_not_given_an_expansion_it_does_not_have() {
        let b = brief(&Playing {
            name: "Factorio",
            running: true,
            facts: factorio::Facts {
                version: Some("1.1.109".into()),
                saves: vec!["freeplay".into()],
                ..factorio::Facts::default()
            },
        })
        .unwrap();
        assert!(!b.contains("Expansions"), "{b}");
        assert!(!b.contains("vanilla one"), "{b}");
    }
}
