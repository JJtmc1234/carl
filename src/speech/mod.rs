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

mod filler;
mod sentences;
mod words;
pub use filler::{Armed, Filler, PHRASES, WAIT};
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

    /// Opens a voice with nothing to say yet.
    ///
    /// Started before the words exist, deliberately. Piper takes about 0.29 seconds to load
    /// its model, measured, and that is per process. Starting one per sentence spent that
    /// again on every sentence and left an audible gap between each. Opening one stream at the
    /// moment Carl decides to answer hides the whole of it behind the five seconds Claude is
    /// already taking, and the sentences then run together the way speech does.
    pub fn open(&self) -> Result<Speaking> {
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
        // its output it blocks partway through while this end is still blocked writing the
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

        let stdin = piper
            .stdin
            .take()
            .ok_or_else(|| Error::Refused("no stdin on piper".into()))?;

        Ok(Speaking {
            piper,
            player,
            stdin: Some(stdin),
            said_anything: false,
        })
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

        let mut stdin = piper
            .stdin
            .take()
            .ok_or_else(|| Error::Refused("no stdin on piper".into()))?;
        writeln!(stdin, "{text}")?;

        Ok(Some(Speaking {
            piper,
            player,
            stdin: Some(stdin),
            said_anything: true,
        }))
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

    pub fn player_path(&self) -> &Path {
        &self.player
    }

    pub fn sink_name(&self) -> Option<&str> {
        self.sink.as_deref()
    }

    /// Synthesises once to a file, for something that has to be instant later.
    ///
    /// Piper takes 0.29 seconds to start, which is most of the gap a filler exists to cover,
    /// so the only way to be quick when it matters is to have done the work already.
    pub fn render(&self, text: &str, to: &Path) -> Result<()> {
        let text = speakable(text);
        if text.is_empty() {
            return Err(Error::Refused("nothing to render".into()));
        }

        let mut piper = Command::new(&self.piper)
            .args([
                "--model".to_string(),
                self.model.to_string_lossy().into_owned(),
                "--output_file".to_string(),
                to.to_string_lossy().into_owned(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot run piper: {e}")))?;

        piper
            .stdin
            .take()
            .ok_or_else(|| Error::Refused("no stdin on piper".into()))?
            .write_all(text.as_bytes())?;

        let done = piper.wait()?;
        if !done.success() {
            return Err(Error::Refused(format!("piper failed: {done}")));
        }
        Ok(())
    }
}

/// Speech in progress, which can be added to, waited for, or cut off.
pub struct Speaking {
    piper: Child,
    player: Child,
    /// Held open so more can be said. Dropping it is what tells piper the answer is over.
    stdin: Option<std::process::ChildStdin>,
    said_anything: bool,
}

impl Speaking {
    /// Adds another sentence to what is being said.
    ///
    /// One line per sentence, because piper synthesises a line at a time and a line break is
    /// what makes it start rather than wait for more.
    pub fn add(&mut self, text: &str) -> Result<()> {
        let text = speakable(text);
        if text.is_empty() {
            return Ok(());
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(Error::Refused("this voice has already finished".into()));
        };
        writeln!(stdin, "{text}")?;
        self.said_anything = true;
        Ok(())
    }

    /// Says that nothing more is coming, without waiting for it to finish.
    ///
    /// Piper reads until end of file, so a stream with its input still open is not finished,
    /// it is waiting. Anything that then asks whether it is done waits forever.
    pub fn close_input(&mut self) {
        self.stdin.take();
    }

    /// Whether anything has been handed to it yet.
    pub fn is_silent(&self) -> bool {
        !self.said_anything
    }
    /// True once the last sample has been played.
    ///
    /// Only meaningful once nothing more will be added, since a stream waiting for its next
    /// sentence has not finished, it is simply quiet.
    pub fn done(&mut self) -> bool {
        matches!(self.player.try_wait(), Ok(Some(_)) | Err(_))
    }

    /// Stops mid word.
    ///
    /// Piper is killed first. Killing only the player leaves piper blocked writing into a
    /// pipe with no reader, and it would sit there until the next reply happened to drain it.
    pub fn stop(&mut self) {
        self.stdin.take();
        let _ = self.piper.kill();
        let _ = self.player.kill();
        let _ = self.piper.wait();
        let _ = self.player.wait();
    }

    /// Waits for the whole thing to be spoken.
    ///
    /// Closing the input is what ends it. Piper reads until end of file, so without this it
    /// waits forever for a sentence that is never coming.
    pub fn finish(&mut self) -> Result<()> {
        self.stdin.take();
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

    /// Piper synthesises a line at a time, so a sentence without a line break sits in the
    /// buffer and is never spoken. That failure is silence, which is indistinguishable from
    /// Carl deciding not to answer.
    #[test]
    fn every_sentence_is_written_as_its_own_line() {
        let v = v();
        assert!(v.piper_args().contains(&"--output_raw".to_string()));
    }

    #[test]
    fn an_empty_answer_produces_no_speech() {
        assert!(v().start("").unwrap().is_none());
        assert!(v().say("").is_ok(), "empty text must not try to run piper");
    }
}
