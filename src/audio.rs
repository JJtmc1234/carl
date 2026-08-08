//! Listening to the microphone without keeping anything.
//!
//! One long running `arecord` writes raw PCM to a pipe, and Carl reads a sliding window out
//! of it. Recording in separate fixed chunks would be simpler and wrong: "Hey Carl" takes
//! about eight tenths of a second, so a wake word landing on a chunk boundary would be cut
//! in half and missed. A sliding window has no boundaries to fall down.
//!
//! Samples live in a ring buffer in memory and in a scratch file under `/dev/shm`, which is
//! RAM. Unlinking a file on the SSD leaves the bytes there until something overwrites them.
//! In RAM there is nothing left to recover, which is what makes the deletion real.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use crate::{Error, Result};

/// Whisper wants 16 kHz mono. Feeding it anything else means resampling, so record it right.
pub const RATE: u32 = 16_000;
const BYTES_PER_SAMPLE: usize = 2;

/// A microphone that is running but keeping nothing beyond the window.
pub struct Mic {
    child: Child,
    /// The last `window` seconds of audio, oldest first.
    ring: Vec<u8>,
    capacity: usize,
    scratch: PathBuf,
}

impl Mic {
    /// Opens the microphone and starts recording into memory.
    ///
    /// `window_secs` is how much of the recent past Carl can see at once. Three seconds is
    /// comfortably longer than a wake word and short enough that idle audio is discarded
    /// almost immediately.
    pub fn open(window_secs: f32, scratch_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(scratch_dir)?;

        let child = Command::new("arecord")
            .args([
                "--quiet",
                "--format", "S16_LE",
                "--rate", &RATE.to_string(),
                "--channels", "1",
                "--file-type", "raw",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot open the microphone via arecord: {e}")))?;

        let capacity = (RATE as f32 * window_secs) as usize * BYTES_PER_SAMPLE;

        Ok(Self {
            child,
            ring: Vec::with_capacity(capacity),
            capacity,
            scratch: scratch_dir.join("listening.wav"),
        })
    }

    /// Reads whatever the microphone has produced, dropping anything older than the window.
    ///
    /// Blocks until at least `at_least_secs` of new audio has arrived, so the caller loops at
    /// the speed of the microphone rather than spinning.
    pub fn advance(&mut self, at_least_secs: f32) -> Result<()> {
        let want = (RATE as f32 * at_least_secs) as usize * BYTES_PER_SAMPLE;
        let mut got = 0;
        let mut buf = vec![0u8; 4096];

        let stdout = self
            .child
            .stdout
            .as_mut()
            .ok_or_else(|| Error::Refused("the microphone pipe closed".into()))?;

        while got < want {
            let n = stdout.read(&mut buf)?;
            if n == 0 {
                return Err(Error::Refused("the microphone stopped".into()));
            }
            self.ring.extend_from_slice(&buf[..n]);
            got += n;
        }

        // Forget everything older than the window. This is the discard that makes the
        // promise true: audio nobody asked Carl about is gone within seconds.
        if self.ring.len() > self.capacity {
            let excess = self.ring.len() - self.capacity;
            self.ring.drain(..excess);
        }
        Ok(())
    }

    /// Writes the current window out as a wav for whisper to read.
    pub fn snapshot(&self) -> Result<&Path> {
        write_wav(&self.scratch, &self.ring)?;
        Ok(&self.scratch)
    }

    /// How loud the window is, as a fraction of full scale.
    ///
    /// A cheap gate in front of whisper. Running a model over silence costs real time for a
    /// guaranteed empty answer, and a room is silent most of the time.
    pub fn loudness(&self) -> f32 {
        if self.ring.len() < BYTES_PER_SAMPLE {
            return 0.0;
        }
        let mut peak = 0i32;
        for pair in self.ring.chunks_exact(BYTES_PER_SAMPLE) {
            let s = i16::from_le_bytes([pair[0], pair[1]]) as i32;
            peak = peak.max(s.abs());
        }
        peak as f32 / i16::MAX as f32
    }

    /// Throws away the window without transcribing it.
    pub fn forget(&mut self) {
        self.ring.clear();
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Overwrite before unlinking. The scratch file is in RAM, but a zero pass costs
        // nothing and means the last thing said is not sitting in a page either.
        if let Ok(len) = std::fs::metadata(&self.scratch).map(|m| m.len())
            && len > 0
        {
            let _ = std::fs::write(&self.scratch, vec![0u8; len as usize]);
        }
        let _ = std::fs::remove_file(&self.scratch);
    }
}

/// A 16 bit mono PCM wav, header written by hand.
///
/// The header is 44 fixed bytes and writing it directly avoids a dependency for something
/// this small.
pub fn write_wav(path: &Path, pcm: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = pcm.len() as u32;
    let byte_rate = RATE * BYTES_PER_SAMPLE as u32;

    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?; // pcm header size
    f.write_all(&1u16.to_le_bytes())?; // format: pcm
    f.write_all(&1u16.to_le_bytes())?; // channels: mono
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&byte_rate.to_le_bytes())?;
    f.write_all(&(BYTES_PER_SAMPLE as u16).to_le_bytes())?; // block align
    f.write_all(&16u16.to_le_bytes())?; // bits per sample
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    f.write_all(pcm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wav_header_is_what_whisper_expects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.wav");
        // A tenth of a second of silence.
        write_wav(&path, &vec![0u8; (RATE as usize / 10) * 2]).unwrap();

        let b = std::fs::read(&path).unwrap();
        assert_eq!(&b[0..4], b"RIFF");
        assert_eq!(&b[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([b[22], b[23]]), 1, "must be mono");
        assert_eq!(
            u32::from_le_bytes([b[24], b[25], b[26], b[27]]),
            RATE,
            "whisper resamples anything else, so get it right at the source"
        );
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), 16, "16 bit samples");
        assert_eq!(b.len(), 44 + (RATE as usize / 10) * 2);
    }

    #[test]
    fn an_empty_recording_still_writes_a_valid_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wav");
        write_wav(&path, &[]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44);
    }
}
