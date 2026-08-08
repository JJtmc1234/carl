//! Carl's voice.
//!
//! Piper, running locally. Measured here at a real time factor of 0.04, so five seconds of
//! speech is generated in about two tenths of a second. Nothing said leaves the machine.
//!
//! Audio is piped straight into the player rather than written to a file and played back.
//! A file would add a write, a read and a process start to every reply, and the point of a
//! spoken assistant is that the answer arrives while you still care about the question.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{Error, Result};

/// Piper's models are 22.05 kHz mono, and the player has to be told so.
const RATE: &str = "22050";

pub struct Voice {
    piper: PathBuf,
    model: PathBuf,
    player: PathBuf,
}

impl Voice {
    pub fn found() -> Result<Self> {
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Refused("no HOME".into()))?
            .join(".local/share/piper");

        let piper = root.join("piper/piper");
        if !piper.exists() {
            return Err(Error::Refused(format!(
                "piper is not installed at {}. See readme.md for the download.",
                piper.display()
            )));
        }
        Ok(Self {
            piper,
            model: root.join("voices/en_US-lessac-medium.onnx"),
            player: PathBuf::from("aplay"),
        })
    }

    pub fn with_paths(
        piper: impl Into<PathBuf>,
        model: impl Into<PathBuf>,
        player: impl Into<PathBuf>,
    ) -> Self {
        Self {
            piper: piper.into(),
            model: model.into(),
            player: player.into(),
        }
    }

    pub fn piper_args(&self) -> Vec<String> {
        vec![
            "--model".into(),
            self.model.to_string_lossy().into_owned(),
            // Raw to stdout, so it can be piped into the player with no file in between.
            "--output_raw".into(),
        ]
    }

    pub fn player_args(&self) -> Vec<String> {
        vec![
            "--quiet".into(),
            "--rate".into(),
            RATE.into(),
            "--format".into(),
            "S16_LE".into(),
            "--channels".into(),
            "1".into(),
            "--file-type".into(),
            "raw".into(),
            "-".into(),
        ]
    }

    /// Says something out loud, returning once it has finished speaking.
    pub fn say(&self, text: &str) -> Result<()> {
        let text = speakable(text);
        if text.is_empty() {
            return Ok(());
        }

        let mut piper = Command::new(&self.piper)
            .args(self.piper_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot run piper: {e}")))?;

        piper
            .stdin
            .take()
            .ok_or_else(|| Error::Refused("no stdin on piper".into()))?
            .write_all(text.as_bytes())?;

        let audio = piper
            .stdout
            .take()
            .ok_or_else(|| Error::Refused("no stdout on piper".into()))?;

        let mut player = Command::new(&self.player)
            .args(self.player_args())
            .stdin(Stdio::from(audio))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot run {}: {e}", self.player.display())))?;

        // Both are waited on. Leaving piper unreaped would pile up a zombie per reply, and a
        // long conversation is a lot of replies.
        let spoke = player.wait()?;
        let _ = piper.wait();

        if !spoke.success() {
            return Err(Error::Refused(format!("playback failed: {spoke}")));
        }
        Ok(())
    }

    pub fn model(&self) -> &Path {
        &self.model
    }
}

/// Turns an answer into something worth hearing out loud.
///
/// Claude writes for a screen. Code fences, bullet markers and bare URLs are all fine to read
/// and dismal to listen to, and a spoken reply that recites a URL character by character is
/// worse than one that skips it.
pub fn speakable(raw: &str) -> String {
    let mut out = Vec::new();
    let mut in_code = false;

    for line in raw.lines() {
        let t = line.trim();

        if t.starts_with("```") {
            in_code = !in_code;
            if in_code {
                out.push("There is a code block on screen.".to_string());
            }
            continue;
        }
        if in_code || t.is_empty() {
            continue;
        }

        // Strip leading list markers and heading hashes. "Dash dash dash" is not speech.
        let t = t
            .trim_start_matches(['#', '-', '*', '>', '+'])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')'])
            .trim();

        if t.is_empty() {
            continue;
        }
        // Inline emphasis and code ticks are punctuation to the eye and noise to the ear.
        out.push(t.replace(['*', '_', '`'], ""));
    }

    out.join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v() -> Voice {
        Voice::with_paths("/opt/piper", "/opt/voices/en.onnx", "/usr/bin/aplay")
    }

    #[test]
    fn piper_streams_raw_so_nothing_hits_a_file() {
        assert!(v().piper_args().contains(&"--output_raw".to_string()));
    }

    /// Piper's models are 22.05 kHz. Playing them at any other rate makes Carl sound like a
    /// chipmunk or a ghost, and it is a one word mistake.
    #[test]
    fn the_player_is_told_pipers_actual_rate() {
        let args = v().player_args();
        let at = args.iter().position(|a| a == "--rate").unwrap();
        assert_eq!(args[at + 1], "22050");
    }

    #[test]
    fn plain_prose_is_left_alone() {
        assert_eq!(
            speakable("Research automation science, then build a second furnace."),
            "Research automation science, then build a second furnace."
        );
    }

    #[test]
    fn list_markers_and_headings_are_not_read_out() {
        assert_eq!(
            speakable("## Next steps\n\n- Get iron\n- Then copper\n1. Finally coal"),
            "Next steps Get iron Then copper Finally coal"
        );
    }

    /// Reading a code block aloud is unbearable, and skipping it silently is confusing, so
    /// Carl says there is one.
    #[test]
    fn code_blocks_are_mentioned_rather_than_recited() {
        let said = speakable("Try this:\n```rust\nlet x = 1;\nlet y = 2;\n```\nThat should work.");
        assert!(said.contains("code block on screen"), "{said}");
        assert!(!said.contains("let x"), "{said}");
        assert!(said.contains("That should work"), "{said}");
    }

    #[test]
    fn inline_markup_is_stripped() {
        assert_eq!(
            speakable("Use the **assembling machine**, not the `furnace`."),
            "Use the assembling machine, not the furnace."
        );
    }

    /// Saying nothing is better than making the speakers pop for an empty string.
    #[test]
    fn an_empty_answer_produces_no_speech() {
        assert_eq!(speakable(""), "");
        assert_eq!(
            speakable("```\ncode only\n```"),
            "There is a code block on screen."
        );
        assert!(v().say("").is_ok(), "empty text must not try to run piper");
    }
}
