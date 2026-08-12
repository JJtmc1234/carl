//! What Carl currently believes about the game in front of him.
//!
//! Separate from `memory/` on purpose, and the reason is the difference between a fact and a
//! state.
//!
//! "JJ's mentor is Hunter" is true next month. "JJ is on blue science" is true for an hour.
//! Putting the second one in permanent memory fills it with things that are quietly false,
//! and a wrong note is worse than a missing one because it comes back every turn and gets
//! stated with confidence.
//!
//! So this is one file per game, replaced rather than appended. There is exactly one current
//! picture, the newest one wins, and nothing accumulates.
//!
//! It is also what makes a second look worth taking. Without it every screenshot is a fresh
//! stranger and "is that better than before" cannot be answered at all.

use std::path::{Path, PathBuf};

/// How much of a picture to keep.
///
/// Small on purpose. This rides along on every turn while the game is up, and a running
/// commentary would cost more than it tells anybody. Enough for where they are, what they are
/// building and what is broken.
pub const LIMIT: usize = 1_200;

/// Where the note for a game lives.
pub fn path(home: &Path, game: &str) -> PathBuf {
    let slug: String = game
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    home.join("game")
        .join(format!("{}.md", slug.trim_matches('-')))
}

pub fn read(home: &Path, game: &str) -> Option<String> {
    let text = std::fs::read_to_string(path(home, game)).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// Replaces the picture. Writing an empty one clears it.
///
/// Replace rather than append, because this is a state and not a log. Two pictures of the
/// same base disagree, and the older one is always the wrong one.
pub fn write(home: &Path, game: &str, picture: &str) -> std::io::Result<()> {
    let file = path(home, game);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let picture = picture.trim();
    if picture.is_empty() {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }

    let mut kept = picture.to_string();
    if kept.len() > LIMIT {
        // Cut on a character boundary, since a picture can hold anything a model wrote.
        let mut cut = LIMIT;
        while cut > 0 && !kept.is_char_boundary(cut) {
            cut -= 1;
        }
        kept.truncate(cut);
    }
    std::fs::write(file, kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_comes_back() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path(), "Factorio"), None, "nothing seen yet");

        write(
            d.path(),
            "Factorio",
            "Red and green science running. No oil yet.",
        )
        .unwrap();
        assert_eq!(
            read(d.path(), "Factorio").as_deref(),
            Some("Red and green science running. No oil yet.")
        );
    }

    /// This is a state and not a log. Two pictures of the same base disagree, and the older
    /// one is always the wrong one.
    #[test]
    fn a_new_picture_replaces_the_old_one() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Factorio", "no oil yet").unwrap();
        write(d.path(), "Factorio", "oil running, starting blue science").unwrap();

        let now = read(d.path(), "Factorio").unwrap();
        assert_eq!(now, "oil running, starting blue science");
        assert!(!now.contains("no oil"), "the old picture must be gone");
    }

    #[test]
    fn an_empty_picture_clears_it() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Factorio", "something").unwrap();
        write(d.path(), "Factorio", "   ").unwrap();
        assert_eq!(read(d.path(), "Factorio"), None);
    }

    #[test]
    fn clearing_something_that_was_never_written_is_fine() {
        let d = tempfile::tempdir().unwrap();
        assert!(write(d.path(), "Factorio", "").is_ok());
    }

    /// Two games are two pictures. A Minecraft base is not a Factorio base.
    #[test]
    fn each_game_has_its_own_picture() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Factorio", "belts").unwrap();
        write(d.path(), "Stardew Valley", "parsnips").unwrap();

        assert_eq!(read(d.path(), "Factorio").as_deref(), Some("belts"));
        assert_eq!(
            read(d.path(), "Stardew Valley").as_deref(),
            Some("parsnips")
        );
    }

    /// A game name becomes a filename, so nothing path shaped may survive.
    #[test]
    fn a_game_name_cannot_smuggle_a_path() {
        let p = path(Path::new("/home/x"), "../../etc/passwd");
        assert!(!p.to_string_lossy().contains(".."), "{}", p.display());
        assert_eq!(p.parent().unwrap(), Path::new("/home/x/game"));
    }

    /// This rides along on every turn while the game is up, so a running commentary would
    /// cost more than it tells anybody.
    #[test]
    fn a_picture_that_grows_forever_is_cut() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "Factorio", &"belts everywhere. ".repeat(500)).unwrap();
        assert!(read(d.path(), "Factorio").unwrap().len() <= LIMIT);
    }

    #[test]
    fn cutting_never_splits_a_character() {
        let d = tempfile::tempdir().unwrap();
        // Three bytes each, so the limit lands mid character unless it is handled.
        write(d.path(), "Factorio", &"日".repeat(LIMIT)).unwrap();
        let back = read(d.path(), "Factorio").unwrap();
        assert!(back.len() <= LIMIT);
        assert!(
            back.chars().all(|c| c == '日'),
            "a character was cut in half"
        );
    }
}
