//! What Carl is allowed to do, and who is asking.
//!
//! Headless has nobody to ask. A tool that is not permitted is refused with no prompt and no
//! way to approve it, so a permission list here is not a convenience, it is the whole of what
//! Carl can do. That is why this is a file JJ owns rather than a constant in the binary.
//!
//! **The surface matters more than the tool.** `Bash(python3:*)` is shell access wearing a hat:
//! it can read anything the user can read. Granting it to the Slack surface hands that to
//! whoever is in the channel. Granting it to the panel hands it to whoever can open a socket in
//! a 0700 directory owned by JJ, which is JJ. Those are not the same risk and one list for both
//! has to be as narrow as the worse of them, which is how the panel ended up unable to run
//! anything.
//!
//! So permits are per surface. The default for every surface is what Carl had before this
//! existed, and widening one says plainly in the file which one is being widened.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Which surface is asking, because they carry different risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The terminal and the panel. Both reach Carl through a path only JJ can use.
    Jj,
    /// Slack, and anything else other people can reach.
    Shared,
}

/// How much Claude decides for itself when a permission is needed.
///
/// The names are the CLI's own, so what is written here is what is passed, and nothing has to
/// translate between two vocabularies that could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    /// Refuse anything not on the list. What Carl did before this existed, and still the
    /// default, because the safe setting is the one you get without choosing.
    #[default]
    Ask,
    /// Allow file edits without asking. Still refuses commands that are not listed.
    AcceptEdits,
    /// Allow everything. Only ever sensible on a surface only JJ can reach, and the file says
    /// so next to it.
    BypassPermissions,
}

impl Mode {
    /// The flag value, or nothing when the default is wanted.
    pub fn flag(self) -> Option<&'static str> {
        match self {
            Mode::Ask => None,
            Mode::AcceptEdits => Some("acceptEdits"),
            Mode::BypassPermissions => Some("bypassPermissions"),
        }
    }
}

/// What one surface may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permits {
    #[serde(default)]
    pub mode: Mode,
    /// Tool patterns, in the CLI's own syntax: `Bash(python3:*)`, `Write`, `Read`.
    #[serde(default = "narrow")]
    pub allow: Vec<String>,
}

fn narrow() -> Vec<String> {
    vec![super::PYTHON.to_string()]
}

impl Default for Permits {
    fn default() -> Self {
        Self {
            mode: Mode::Ask,
            allow: narrow(),
        }
    }
}

/// Both surfaces, as read from disk.
/// Narrows an allow list to what a rank is permitted to hold.
///
/// **Rank narrows, never widens.** Whatever JJ wrote in `permissions.json` is a ceiling, and
/// this brings it down to what the organisation says the agent may do. A permit for a tool the
/// rank forbids is dropped rather than honoured.
///
/// This exists because Carl had two sets of powers. In the chain he is the chief and
/// `tools_for` gives him nothing, which is the whole point of a chief. Reached through the
/// panel, the terminal or Slack he was built from `permissions.json` instead, which grants
/// `Write` and `Edit`, so the agent who is never supposed to implement anything was writing
/// code. Same agent, two answers, and the permissive one was the one JJ talked to.
pub fn narrow_to_rank(allow: &[String], rank: crate::army::org::Rank) -> Vec<String> {
    let permitted = crate::army::chain::tools_for(rank);
    if permitted.is_empty() {
        // A chief holds nothing. Not a short list, nothing.
        return Vec::new();
    }
    allow
        .iter()
        .filter(|wanted| {
            // A permit names a tool, sometimes with an argument pattern like `Bash(python3:*)`.
            // Compare on the tool, so a narrowed `Bash` still admits the specific Bash permits
            // JJ wrote and a forbidden `Write` is dropped however it was spelled.
            let tool = wanted.split(['(', ' ']).next().unwrap_or(wanted);
            permitted.iter().any(|p| p == tool)
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Book {
    /// The terminal and the panel.
    #[serde(default)]
    pub jj: Permits,
    /// Slack, and anything else other people can reach.
    #[serde(default)]
    pub shared: Permits,
}

impl Book {
    /// Reads `<home>/permissions.json`.
    ///
    /// A missing file is the default rather than an error, so nothing has to be created before
    /// Carl will run. A file that will not parse **is** an error, because the alternative is
    /// falling back to a different permission set than the one somebody wrote down, and being
    /// quietly more or less permitted than intended is the worst outcome available here.
    pub fn load(home: &Path) -> crate::Result<Self> {
        let path = Self::path(home);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };
        serde_json::from_str(&text).map_err(|e| {
            crate::Error::Refused(format!(
                "{} will not parse: {e}. Fix it or delete it; Carl will not guess at what \
                 somebody meant to permit.",
                path.display()
            ))
        })
    }

    pub fn path(home: &Path) -> std::path::PathBuf {
        home.join("permissions.json")
    }

    pub fn for_surface(&self, surface: Surface) -> &Permits {
        match surface {
            Surface::Jj => &self.jj,
            Surface::Shared => &self.shared,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_home_with_no_file_is_what_carl_had_before() {
        let d = tempfile::tempdir().unwrap();
        let book = Book::load(d.path()).unwrap();
        assert_eq!(book.jj, Permits::default());
        assert_eq!(book.shared, Permits::default());
        assert_eq!(book.jj.allow, vec![crate::claude::PYTHON.to_string()]);
        assert_eq!(
            book.jj.mode,
            Mode::Ask,
            "the safe setting is the one you get"
        );
    }

    /// The whole point of splitting them. Widening the panel must not widen Slack.
    #[test]
    fn widening_one_surface_leaves_the_other_alone() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            Book::path(d.path()),
            r#"{"jj":{"mode":"acceptEdits","allow":["Bash(python3:*)","Write","Read"]}}"#,
        )
        .unwrap();

        let book = Book::load(d.path()).unwrap();
        assert_eq!(book.jj.mode, Mode::AcceptEdits);
        assert!(book.jj.allow.contains(&"Write".to_string()));
        assert_eq!(
            book.shared,
            Permits::default(),
            "slack keeps the narrow list nobody widened"
        );
    }

    /// Falling back to a default that is not what somebody wrote is the worst outcome here:
    /// Carl would be quietly more or less permitted than intended and nothing would say so.
    #[test]
    fn a_file_that_will_not_parse_is_an_error_rather_than_a_guess() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(Book::path(d.path()), "{ not json").unwrap();
        let e = Book::load(d.path()).unwrap_err().to_string();
        assert!(e.contains("permissions.json"), "{e}");
        assert!(e.contains("will not guess"), "{e}");
    }

    #[test]
    fn only_a_raised_mode_reaches_the_command_line() {
        assert_eq!(Mode::Ask.flag(), None, "the default is not passed twice");
        assert_eq!(Mode::AcceptEdits.flag(), Some("acceptEdits"));
        assert_eq!(Mode::BypassPermissions.flag(), Some("bypassPermissions"));
    }
}
