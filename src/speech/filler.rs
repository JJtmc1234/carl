//! The small noise a person makes while they think.
//!
//! Carl takes a second or two to answer, which is fast for a model and long for a silence. A
//! person asked a question does not go completely quiet, they say "mm" or "right" and then
//! answer, and the sound is not the point. The point is that you know you were heard.
//!
//! Two things make this worth doing properly rather than badly.
//!
//! It has to be instant, so it cannot be synthesised on demand. Piper takes 0.29 seconds to
//! start, measured, which is most of the gap it is supposed to be covering. So the phrases are
//! rendered once at startup and played from a file afterwards, which costs the time to start
//! `aplay` and nothing else.
//!
//! And it has to not fire when the answer is quick. A filler followed immediately by the
//! answer is worse than the silence it replaced, because it sounds like a stutter. So nothing
//! is played until the answer is actually late.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// How long a silence has to last before it is worth covering.
///
/// Under this, saying anything makes it worse. The measured wait to a first token is about a
/// second on a warm process, so this fires on most turns and not on the fast ones.
pub const WAIT: std::time::Duration = std::time::Duration::from_millis(450);

/// What Carl says while he thinks.
///
/// Short, and none of them promise anything. "Let me check" is a claim about what he is doing
/// and is sometimes false. "Mm" is only an acknowledgement, which is all this is for.
pub const PHRASES: &[&str] = &["Mm.", "Right.", "Hm.", "Okay.", "Let me think."];

pub struct Filler {
    ready: Vec<PathBuf>,
    next: usize,
    player: PathBuf,
    sink: Option<String>,
}

impl Filler {
    /// Renders the phrases once, so playing one later is only a file read.
    ///
    /// Failing to render is not fatal anywhere. A Carl with no filler is the Carl that existed
    /// before this, and losing the whole voice because a nicety would not synthesise would be
    /// a poor trade.
    pub fn prepare(voice: &super::Voice, scratch: &Path) -> Self {
        let mut ready = Vec::new();
        std::fs::create_dir_all(scratch).ok();

        for (i, phrase) in PHRASES.iter().enumerate() {
            let path = scratch.join(format!("filler-{i}.wav"));
            match voice.render(phrase, &path) {
                Ok(()) => ready.push(path),
                Err(e) => eprintln!("could not prepare a filler: {e}"),
            }
        }

        Self {
            ready,
            next: 0,
            player: voice.player_path().to_path_buf(),
            sink: voice.sink_name().map(str::to_owned),
        }
    }

    pub fn is_ready(&self) -> bool {
        !self.ready.is_empty()
    }

    /// Sets a noise to play shortly, unless the answer arrives first.
    ///
    /// Armed rather than played, because the whole point is to make a sound while nothing is
    /// happening, and while nothing is happening Carl is blocked waiting for a model. So a
    /// thread holds the timer and the first word of the answer cancels it.
    ///
    /// Rotates rather than choosing at random. The same sound twice running is what makes a
    /// filler noticeable, and rotating cannot produce one.
    pub fn arm(&mut self) -> Armed {
        let cancel = Arc::new(AtomicBool::new(false));
        if self.ready.is_empty() {
            return Armed { cancel };
        }

        let path = self.ready[self.next % self.ready.len()].clone();
        self.next = self.next.wrapping_add(1);

        let player = self.player.clone();
        let sink = self.sink.clone();
        let flag = cancel.clone();

        std::thread::spawn(move || {
            std::thread::sleep(WAIT);
            if flag.load(Ordering::Relaxed) {
                return;
            }
            let mut cmd = Command::new(&player);
            if sink.is_some() {
                cmd.args(["-D", "pulse"]);
            }
            cmd.arg("--quiet").arg(&path);
            if let Some(sink) = &sink {
                cmd.env("PULSE_SINK", sink);
            }
            // Waited for, so the process is reaped. A filler per turn is a zombie per turn
            // otherwise, and a long conversation is a lot of turns.
            if let Ok(mut child) = cmd.stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
                let _ = child.wait();
            }
        });

        Armed { cancel }
    }
}

/// A filler that has not happened yet.
///
/// Cancelled by the first word of the answer, or by being dropped, so an answer that arrives
/// quickly is never followed by a noise about how long it took.
pub struct Armed {
    cancel: Arc<AtomicBool>,
}

impl Armed {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for Armed {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filler followed straight away by the answer sounds like a stutter, which is worse
    /// than the silence it replaced.
    #[test]
    fn nothing_is_said_before_the_silence_is_worth_covering() {
        assert!(WAIT >= std::time::Duration::from_millis(300));
    }

    /// None of these may promise anything. "Let me check the map" is a claim about what Carl
    /// is doing, and he is not doing it, he is waiting for a model.
    #[test]
    fn the_phrases_are_acknowledgements_and_not_claims() {
        for p in PHRASES {
            assert!(p.len() < 20, "{p} is too long to be a noise");
            let l = p.to_lowercase();
            for claim in ["check", "look", "search", "find", "run"] {
                assert!(!l.contains(claim), "{p} promises to {claim}");
            }
        }
    }

    /// The same sound twice running is what makes a filler noticeable, and rotating cannot
    /// produce one.
    #[test]
    fn the_same_one_is_never_played_twice_in_a_row() {
        let mut f = Filler {
            ready: vec![PathBuf::from("a"), PathBuf::from("b")],
            next: 0,
            player: PathBuf::from("/bin/true"),
            sink: None,
        };
        let mut seen = Vec::new();
        for _ in 0..6 {
            let i = f.next % f.ready.len();
            seen.push(f.ready[i].clone());
            f.next = f.next.wrapping_add(1);
        }
        assert!(
            seen.windows(2).all(|w| w[0] != w[1]),
            "a repeat slipped through: {seen:?}"
        );
    }

    /// Nothing rendered means no filler, not a crash. A Carl with no filler is the Carl that
    /// existed before this.
    #[test]
    fn no_phrases_means_no_sound_rather_than_an_error() {
        let mut f = Filler {
            ready: Vec::new(),
            next: 0,
            player: PathBuf::from("/nonexistent"),
            sink: None,
        };
        let armed = f.arm();
        armed.cancel();
        assert!(!f.is_ready());
    }

    /// An answer that arrives quickly must never be followed by a noise about how long it
    /// took, which is what dropping without firing guarantees.
    #[test]
    fn dropping_it_cancels_it() {
        let mut f = Filler {
            ready: vec![PathBuf::from("/nonexistent.wav")],
            next: 0,
            player: PathBuf::from("/bin/true"),
            sink: None,
        };
        let armed = f.arm();
        let flag = armed.cancel.clone();
        drop(armed);
        assert!(flag.load(Ordering::Relaxed), "drop must cancel");
    }
}
