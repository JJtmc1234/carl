//! Turning recorded audio into text.
//!
//! Two models, on purpose. The wake word check runs constantly on every window of audio, so
//! it has to be nearly free. The conversation tier only runs once you have actually said
//! something to Carl, so it can afford to be better.
//!
//! Measured on this machine, CPU only, on a real spoken question lasting 1.65 seconds. All
//! three transcribed it identically, word for word.
//!
//! | model | time | used for |
//! |---|---|---|
//! | `tiny.en` | 0.49s | the wake word, running all the time |
//! | `base.en` | 0.89s | the conversation |
//! | `small.en` | 2.84s | the conversation with `--accurate` |
//! | `large-v3-turbo` | 13.9s for 11s of speech | nothing. Too slow on CPU to sit in a loop. |
//!
//! Of that 0.89 seconds, only 0.15 is loading the model and 0.59 is the actual encoding, so
//! keeping whisper resident would save less than it looks like. The thread count was worth
//! more, see `threads`.
//!
//! `base.en` rather than `small.en` for the conversation, which is a two second saving on
//! every single thing you say. Two seconds is a long time to wait having already finished
//! talking, and on clean audio the better model was not buying anything. The echo canceller
//! suppresses room noise on the way through, so the audio reaching this is clean now in a
//! way it was not when `small.en` was chosen. `--accurate` puts it back for a noisy room or
//! a mouthful of item names.

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
    /// Trade two seconds a turn for the larger conversation model.
    accurate: bool,
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
            accurate: false,
        })
    }

    pub fn with_paths(binary: impl Into<PathBuf>, models: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            models: models.into(),
            accurate: false,
        }
    }

    /// Uses the larger conversation model, at about two seconds a turn.
    pub fn accurate(mut self, yes: bool) -> Self {
        self.accurate = yes;
        self
    }

    fn model_for(&self, tier: Tier) -> PathBuf {
        self.models.join(match tier {
            Tier::Wake => "ggml-tiny.en.bin",
            Tier::Talk if self.accurate => "ggml-small.en.bin",
            Tier::Talk => "ggml-base.en.bin",
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
            "--threads".into(),
            threads().to_string(),
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

/// How many threads to transcribe with.
///
/// Three quarters of the machine, which was measured rather than reasoned about. The first
/// version used every core but one, on the theory that leaving one free is polite. On this
/// machine that is slower, and not slightly.
///
/// | threads | time |
/// |---|---|
/// | 4 | 845ms |
/// | 8 | 709ms |
/// | 12 | 636ms |
/// | 16 | 685ms |
///
/// The reason is that a modern desktop chip does not have sixteen of the same core. This one
/// has fast cores and slow ones, and whisper splits its work into equal pieces, so every
/// piece finishes at the speed of the slowest core it landed on. Handing work to the slow
/// cores makes the whole batch wait for them.
///
/// Three quarters is a rule rather than the number 12, because the next machine will have a
/// different count and the same shape of problem.
pub fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() * 3 / 4).max(2))
        .unwrap_or(4)
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
        assert!(args.iter().any(|a| a.contains("base.en")), "{args:?}");
        // large-v3-turbo took 13.9s for 11s of audio here. It must not creep back in.
        assert!(!args.iter().any(|a| a.contains("large")), "{args:?}");
    }

    /// Measured, not reasoned about. Using every core is slower on a machine with fast and
    /// slow cores mixed, because whisper splits its work evenly and the batch finishes at the
    /// speed of the slowest piece.
    #[test]
    fn transcription_uses_three_quarters_of_the_machine() {
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let t = threads();
        assert!(t >= 2, "always at least two");
        assert!(t <= cores, "never more than the machine has");
        if cores >= 8 {
            assert!(t < cores, "using every core measured slower");
        }
    }

    #[test]
    fn the_thread_count_reaches_the_command_line() {
        let w = Whisper::with_paths("/opt/whisper", "/opt/models");
        let args = w.args_for(Tier::Talk, Path::new("/tmp/a.wav"));
        let at = args.iter().position(|a| a == "--threads").unwrap();
        assert_eq!(args[at + 1], threads().to_string());
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
