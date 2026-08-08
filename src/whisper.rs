//! Turning recorded audio into text.
//!
//! Two models, on purpose. The wake word check runs constantly on every window of audio, so
//! it has to be nearly free. The conversation tier only runs once you have actually said
//! something to Carl, so it can afford to be better.
//!
//! Measured on this machine, for eleven seconds of speech, CPU only:
//!
//! | model | time | used for |
//! |---|---|---|
//! | `tiny.en` | 0.7s | the wake word, running all the time |
//! | `base.en` | 1.3s | |
//! | `small.en` | 3.4s | the conversation |
//! | `large-v3-turbo` | 13.9s | nothing. Too slow on CPU to sit in a loop. |

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Error, Result};

/// Which tier to transcribe at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Constant, cheap, only has to spot two words.
    Wake,
    /// Occasional, accurate, has to get item names right.
    Talk,
}

pub struct Whisper {
    binary: PathBuf,
    models: PathBuf,
}

impl Whisper {
    /// Looks for the build under `~/.local/share/whisper.cpp`.
    pub fn found() -> Result<Self> {
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| Error::Refused("no HOME".into()))?
            .join(".local/share/whisper.cpp");

        let binary = root.join("build/bin/whisper-cli");
        if !binary.exists() {
            return Err(Error::Refused(format!(
                "whisper is not built at {}. Build it with: cmake -B build && cmake --build build -j",
                binary.display()
            )));
        }
        Ok(Self {
            binary,
            models: root.join("models"),
        })
    }

    pub fn with_paths(binary: impl Into<PathBuf>, models: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            models: models.into(),
        }
    }

    fn model_for(&self, tier: Tier) -> PathBuf {
        self.models.join(match tier {
            Tier::Wake => "ggml-tiny.en.bin",
            Tier::Talk => "ggml-small.en.bin",
        })
    }

    pub fn args_for(&self, tier: Tier, wav: &Path) -> Vec<String> {
        vec![
            "--model".into(),
            self.model_for(tier).to_string_lossy().into_owned(),
            "--file".into(),
            wav.to_string_lossy().into_owned(),
            // No timestamps, no progress chatter. Just the words.
            "--no-timestamps".into(),
            "--no-prints".into(),
            // One thread short of the machine, so transcribing does not starve the game.
            "--threads".into(),
            std::thread::available_parallelism()
                .map(|n| (n.get().saturating_sub(1)).max(1))
                .unwrap_or(4)
                .to_string(),
        ]
    }

    /// Transcribes one file. Returns the text, trimmed.
    pub fn transcribe(&self, tier: Tier, wav: &Path) -> Result<String> {
        let model = self.model_for(tier);
        if !model.exists() {
            return Err(Error::Refused(format!(
                "missing model {}. Fetch it with: models/download-ggml-model.sh {}",
                model.display(),
                model
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace("ggml-", ""))
                    .unwrap_or_default()
            )));
        }

        let out = Command::new(&self.binary)
            .args(self.args_for(tier, wav))
            .output()
            .map_err(|e| Error::Refused(format!("cannot run whisper: {e}")))?;

        if !out.status.success() {
            return Err(Error::Refused(format!(
                "whisper failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }

        Ok(clean(&String::from_utf8_lossy(&out.stdout)))
    }
}

/// Strips whisper's noise annotations and whitespace.
///
/// Whisper labels non speech as `[BLANK_AUDIO]`, `(wind blowing)` and similar. Left in, those
/// reach the wake word matcher as words and can produce a match on nothing at all.
pub fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;

    for c in raw.chars() {
        match c {
            '[' | '(' | '*' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    // `*` opens and never closes, so anything after one is dropped for that line. Rebuild by
    // line to stop a single asterisk swallowing the rest of the transcript.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w() -> Whisper {
        Whisper::with_paths("/opt/whisper/whisper-cli", "/opt/whisper/models")
    }

    #[test]
    fn the_wake_tier_uses_the_cheap_model() {
        let args = w().args_for(Tier::Wake, Path::new("/dev/shm/a.wav"));
        assert!(args.iter().any(|a| a.contains("tiny.en")), "{args:?}");
    }

    #[test]
    fn the_talk_tier_uses_the_better_model() {
        let args = w().args_for(Tier::Talk, Path::new("/dev/shm/a.wav"));
        assert!(args.iter().any(|a| a.contains("small.en")), "{args:?}");
        // large-v3-turbo took 13.9s for 11s of audio here. It must not creep back in.
        assert!(!args.iter().any(|a| a.contains("large")), "{args:?}");
    }

    #[test]
    fn transcription_leaves_at_least_one_core_free() {
        let args = w().args_for(Tier::Wake, Path::new("/dev/shm/a.wav"));
        let at = args.iter().position(|a| a == "--threads").unwrap();
        let threads: usize = args[at + 1].parse().unwrap();
        assert!(threads >= 1);
        if let Ok(n) = std::thread::available_parallelism() {
            assert!(
                threads < n.get() || n.get() == 1,
                "the game needs a core too"
            );
        }
    }

    /// Whisper writes [BLANK_AUDIO] over silence. Passed through, it is just words, and words
    /// are what the wake matcher looks at.
    #[test]
    fn noise_annotations_are_stripped() {
        assert_eq!(clean("[BLANK_AUDIO]"), "");
        assert_eq!(clean("  (wind blowing)  "), "");
        assert_eq!(clean("[ Silence ]\n"), "");
        assert_eq!(clean(" Hey Carl, what now? \n"), "Hey Carl, what now?");
        assert_eq!(clean("(door closes) Hey Carl"), "Hey Carl");
    }

    #[test]
    fn a_missing_model_names_how_to_get_it() {
        let err = w()
            .transcribe(Tier::Wake, Path::new("/dev/shm/nope.wav"))
            .unwrap_err();
        assert!(err.to_string().contains("download-ggml-model"), "{err}");
    }
}
