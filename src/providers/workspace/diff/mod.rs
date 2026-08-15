//! Showing what changed, without changing anything.
//!
//! Two sources, because they answer different questions.
//!
//! **Git** answers what changed against the branch. It is asked through a fixed list of read
//! only subcommands, and `run` refuses anything not on that list. That is not because a typo
//! is likely, it is because "just check the file out to compare it" is a genuinely tempting way
//! to implement a diff and it destroys work. There is no code path here that can write to a
//! repository, so the temptation cannot be acted on later by somebody in a hurry.
//!
//! **A plain text comparison** answers what changed in the editor before saving, which git
//! cannot see because the buffer is not on disk yet.
//!
//! The text comparison is deliberately simple: it trims the common start and end and reports
//! the middle. It is not Myers and does not pretend to be, so on a file that has been heavily
//! rearranged it will show a larger block than a real diff tool would. For "what am I about to
//! save over" that is honest and enough, and when a proper diff is wanted, git already
//! produces one.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// The only git subcommands this module will run.
///
/// Every one of them reads. Nothing here checks out, stashes, resets or commits.
const READ_ONLY: &[&str] = &["diff", "status", "show", "rev-parse", "log"];

/// One changed path, as git reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The two letter code from `git status --porcelain`, for example ` M` or `??`.
    pub status: String,
    pub path: PathBuf,
}

impl Change {
    pub fn is_untracked(&self) -> bool {
        self.status.trim() == "??"
    }
}

/// Runs a read only git command in `repo`.
fn run(repo: &Path, args: &[&str]) -> Result<String> {
    let Some(subcommand) = args.first() else {
        return Err(Error::Refused("no git subcommand was given".into()));
    };
    if !READ_ONLY.contains(subcommand) {
        return Err(Error::Refused(format!(
            "{subcommand} is not one of the read only git commands this runs: {}",
            READ_ONLY.join(", ")
        )));
    }

    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| Error::Refused(format!("could not run git: {e}")))?;

    if !out.status.success() {
        return Err(Error::Refused(format!(
            "git {} failed: {}",
            subcommand,
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The repository a path belongs to, if any.
pub fn repository_of(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    let top = run(&start, &["rev-parse", "--show-toplevel"]).ok()?;
    let top = top.trim();
    (!top.is_empty()).then(|| PathBuf::from(top))
}

/// What one file looks like against the commit the branch is on.
///
/// Empty when the file matches `HEAD`, which is git's own way of saying nothing changed.
pub fn against_head(repo: &Path, path: &Path) -> Result<String> {
    let arg = path.to_string_lossy().into_owned();
    run(repo, &["diff", "--no-color", "HEAD", "--", &arg])
}

/// Everything changed in a working tree, which is how a task branch is inspected.
pub fn worktree_changes(repo: &Path) -> Result<Vec<Change>> {
    let text = run(repo, &["status", "--porcelain"])?;
    Ok(text
        .lines()
        .filter(|l| l.len() > 3)
        .map(|l| Change {
            status: l[..2].to_string(),
            // Renames arrive as `old -> new`, and the new name is the one worth showing.
            path: PathBuf::from(match l[3..].split_once(" -> ") {
                Some((_, new)) => new,
                None => &l[3..],
            }),
        })
        .collect())
}

/// A one line summary of a working tree, for a project row in the panel.
pub fn summarise(repo: &Path) -> Result<String> {
    let changes = worktree_changes(repo)?;
    if changes.is_empty() {
        return Ok("no local changes".to_string());
    }
    let untracked = changes.iter().filter(|c| c.is_untracked()).count();
    let tracked = changes.len() - untracked;
    Ok(match (tracked, untracked) {
        (0, u) => format!("{u} untracked"),
        (t, 0) => format!("{t} changed"),
        (t, u) => format!("{t} changed, {u} untracked"),
    })
}

/// What is in the editor against what is on disk.
pub fn buffer_vs_disk(on_disk: &str, buffer: &str) -> String {
    simple_diff(on_disk, buffer)
}

/// A readable difference between two pieces of text.
///
/// Trims the identical start and end, then shows what is left as removed and added blocks.
pub fn simple_diff(before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }

    let old: Vec<&str> = before.lines().collect();
    let new: Vec<&str> = after.lines().collect();

    let head = old.iter().zip(&new).take_while(|(a, b)| a == b).count();

    // The tail is measured on what is left after the head, so a short file cannot have the
    // same line counted at both ends.
    let most = old.len().min(new.len()) - head;
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count()
        .min(most);

    let mut out = String::new();
    out.push_str(&format!("@@ line {} @@\n", head + 1));
    for line in &old[head..old.len() - tail] {
        out.push_str(&format!("-{line}\n"));
    }
    for line in &new[head..new.len() - tail] {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

#[cfg(test)]
mod tests;
