//! Carl's voice.
//!
//! Piper, running locally. Measured here at a real time factor of 0.04, so five seconds of
//! speech is generated in about two tenths of a second. Nothing said leaves the machine.
//!
//! Audio is piped straight into the player rather than written to a file and played back.
//! A file would add a write, a read and a process start to every reply, and the point of a
//! spoken assistant is that the answer arrives while you still care about the question.
//!
//! Speech is startable and stoppable rather than one blocking call, because being able to
//! interrupt Carl mid sentence is most of what separates this from a machine reading at you.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

mod sentences;
mod words;
pub use sentences::Sentences;
pub use words::speakable;

use crate::{Error, Result};

/// Piper's models are 22.05 kHz mono, and the player has to be told so.
const RATE: &str = "22050";

pub struct Voice {
    piper: PathBuf,
    model: PathBuf,
    player: PathBuf,
    /// Which sink to play into. The echo cancelled one when it exists, so the canceller knows
    /// what sound is leaving the machine and can subtract it from the microphone.
    sink: Option<String>,
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
            sink: None,
        })
    }

    /// Sends Carl's voice to a named sink instead of the default one.
    pub fn to_sink(mut self, sink: Option<&str>) -> Self {
        self.sink = sink.map(str::to_owned);
        self
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
            sink: None,
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
        let mut args: Vec<String> = Vec::new();
        if self.sink.is_some() {
            args.extend(["-D".to_string(), "pulse".to_string()]);
        }
        args.extend(
            [
                "--quiet",
                "--rate",
                RATE,
                "--format",
                "S16_LE",
                "--channels",
                "1",
                "--file-type",
                "raw",
                "-",
            ]
            .map(str::to_owned),
        );
        args
    }

    /// Begins speaking and returns immediately. `None` means there was nothing to say.
    pub fn start(&self, text: &str) -> Result<Option<Speaking>> {
        let text = speakable(text);
        if text.is_empty() {
            return Ok(None);
        }

        let mut piper = Command::new(&self.piper)
            .args(self.piper_args())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot run piper: {e}")))?;

        let audio = piper
            .stdout
            .take()
            .ok_or_else(|| Error::Refused("no stdout on piper".into()))?;

        // The player starts before a single byte of text is written, and that order is not
        // cosmetic. Piper generates far more audio than a pipe holds, so with nothing reading
        // its output it blocks partway through, while this end is still blocked writing the
        // input. Two processes each waiting for the other, on any answer past a paragraph.
        let mut player = Command::new(&self.player);
        player.args(self.player_args());
        if let Some(sink) = &self.sink {
            player.env("PULSE_SINK", sink);
        }
        let player = player
            .stdin(Stdio::from(audio))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot run {}: {e}", self.player.display())))?;

        piper
            .stdin
            .take()
            .ok_or_else(|| Error::Refused("no stdin on piper".into()))?
            .write_all(text.as_bytes())?;

        Ok(Some(Speaking { piper, player }))
    }

    /// Says something out loud, returning once it has finished speaking.
    pub fn say(&self, text: &str) -> Result<()> {
        match self.start(text)? {
            Some(mut s) => s.finish(),
            None => Ok(()),
        }
    }

    pub fn model(&self) -> &Path {
        &self.model
    }
}

/// Speech in progress, which can be waited for or cut off.
pub struct Speaking {
    piper: Child,
    player: Child,
}

impl Speaking {
    /// True once the last sample has been played.
    pub fn done(&mut self) -> bool {
        matches!(self.player.try_wait(), Ok(Some(_)) | Err(_))
    }

    /// Stops mid word.
    ///
    /// Piper is killed first. Killing only the player leaves piper blocked writing into a
    /// pipe with no reader, and it would sit there until the next reply happened to drain it.
    pub fn stop(&mut self) {
        let _ = self.piper.kill();
        let _ = self.player.kill();
        let _ = self.piper.wait();
        let _ = self.player.wait();
    }

    /// Waits for the whole thing to be spoken.
    pub fn finish(&mut self) -> Result<()> {
        let spoke = self.player.wait()?;
        // Both are reaped. Leaving piper unreaped piles up a zombie per reply, and a long
        // conversation is a lot of replies.
        let _ = self.piper.wait();

        if !spoke.success() {
            return Err(Error::Refused(format!("playback failed: {spoke}")));
        }
        Ok(())
    }
}

impl Drop for Speaking {
    fn drop(&mut self) {
        if !self.done() {
            self.stop();
        }
    }
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

    /// Only the named sink route asks for the pulse device. Adding it unconditionally would
    /// break a machine with no PulseAudio at all.
    #[test]
    fn the_pulse_device_appears_only_when_a_sink_is_named() {
        assert!(!v().player_args().contains(&"pulse".to_string()));

        let routed = v().to_sink(Some("carl-speaker"));
        let args = routed.player_args();
        let at = args.iter().position(|a| a == "-D").expect("no -D");
        assert_eq!(args[at + 1], "pulse");
        // The rate still has to survive the extra arguments.
        let at = args.iter().position(|a| a == "--rate").unwrap();
        assert_eq!(args[at + 1], "22050");
    }

    #[test]
    fn an_empty_answer_produces_no_speech() {
        assert!(v().start("").unwrap().is_none());
        assert!(v().say("").is_ok(), "empty text must not try to run piper");
    }
}
