//! Listening to the microphone without keeping anything, and without falling behind.
//!
//! One long lived `arecord` writes raw PCM to a pipe. A reader thread drains that pipe as
//! fast as it arrives into a ring buffer holding only the last few seconds.
//!
//! The thread is the whole point. Reading the pipe from the main loop seems simpler and is
//! badly wrong: transcribing takes time, answering takes far more, and while either runs
//! nothing is draining the pipe. The reader then falls behind real time by a little every
//! pass and by twenty seconds after one answer, so Carl ends up transcribing a window from
//! minutes ago. It looks exactly like a microphone that does not work.
//!
//! Samples live in memory and in a scratch file under `/dev/shm`, which is RAM. Unlinking a
//! file on the SSD leaves the bytes there until something overwrites them. In RAM there is
//! nothing left to recover, which is what makes the deletion real.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{Error, Result};

/// Whisper wants 16 kHz mono. Feeding it anything else means resampling, so record it right.
pub const RATE: u32 = 16_000;
const BYTES_PER_SAMPLE: usize = 2;

/// Anything below this is a quiet room rather than speech.
pub const SPEECH_FLOOR: f32 = 0.02;

#[derive(Default)]
struct Shared {
    buf: VecDeque<u8>,
    /// Every byte the microphone has ever produced. Lets a reader tell "nothing new yet"
    /// apart from "I fell behind and lost some".
    total: u64,
}

pub struct Mic {
    child: Child,
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
    scratch: PathBuf,
}

impl Mic {
    pub fn open(window_secs: f32, scratch_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(scratch_dir)?;
        let cap = (RATE as f32 * window_secs) as usize * BYTES_PER_SAMPLE;

        let mut child = Command::new("arecord")
            .args([
                "--quiet",
                "--format",
                "S16_LE",
                "--rate",
                &RATE.to_string(),
                "--channels",
                "1",
                "--file-type",
                "raw",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error::Refused(format!("cannot open the microphone via arecord: {e}")))?;

        let mut pipe = child
            .stdout
            .take()
            .ok_or_else(|| Error::Refused("no stdout on arecord".into()))?;

        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));

        let reader = {
            let shared = shared.clone();
            let stop = stop.clone();
            std::thread::spawn(move || {
                let mut chunk = vec![0u8; 4096];
                while !stop.load(Ordering::Relaxed) {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                            s.buf.extend(&chunk[..n]);
                            s.total += n as u64;
                            // Only the recent past is kept. This is the discard that makes
                            // the promise true, and it happens whether anyone is listening
                            // or not.
                            while s.buf.len() > cap {
                                s.buf.pop_front();
                            }
                        }
                    }
                }
            })
        };

        Ok(Self {
            child,
            shared,
            stop,
            reader: Some(reader),
            scratch: scratch_dir.join("listening.wav"),
        })
    }

    /// The most recent audio, however long the caller was away.
    fn window(&self) -> (Vec<u8>, u64) {
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        (s.buf.iter().copied().collect(), s.total)
    }

    /// Waits until at least `secs` of new audio has arrived since the last look.
    ///
    /// Sleeps rather than reading, because the reader thread owns the pipe now. Nothing here
    /// can fall behind: the window is always the most recent few seconds by construction.
    pub fn wait(&self, secs: f32) {
        let want = (RATE as f32 * secs) as u64 * BYTES_PER_SAMPLE as u64;
        let start = self.shared.lock().unwrap_or_else(|e| e.into_inner()).total;
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let now = self.shared.lock().unwrap_or_else(|e| e.into_inner()).total;
            if now.saturating_sub(start) >= want {
                return;
            }
        }
    }

    pub fn loudness(&self) -> f32 {
        peak_of(&self.window().0)
    }

    /// Writes the current window out for whisper to read.
    pub fn snapshot(&self) -> Result<&Path> {
        write_wav(&self.scratch, &self.window().0)?;
        Ok(&self.scratch)
    }

    /// Forgets the window, so the next look starts from now.
    pub fn forget(&self) {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .buf
            .clear();
    }

    /// Records one whole thing you said, from now until you stop talking.
    ///
    /// The window is right for spotting a wake word and wrong for a sentence, which can run
    /// longer than the window and would lose its front. This grows instead.
    pub fn utterance(&self, hush_secs: f32, cap_secs: f32) -> Result<&Path> {
        let hush = (RATE as f32 * hush_secs) as usize * BYTES_PER_SAMPLE;
        let deadline = Instant::now() + Duration::from_secs_f32(cap_secs);

        // Start from whatever is already buffered. The first syllable usually lands before
        // the loop gets here, and losing it costs the whole word.
        let (mut pcm, mut seen) = self.window();

        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(80));

            let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let fresh = s.total.saturating_sub(seen) as usize;
            if fresh > 0 {
                let take = fresh.min(s.buf.len());
                pcm.extend(s.buf.iter().rev().take(take).rev().copied());
                seen = s.total;
            }
            drop(s);

            if pcm.len() > hush * 2 && peak_of(&pcm[pcm.len() - hush..]) < SPEECH_FLOOR {
                break;
            }
        }

        write_wav(&self.scratch, &pcm)?;
        Ok(&self.scratch)
    }
}

impl Drop for Mic {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        // Overwrite before unlinking. The file is in RAM, but a zero pass costs nothing and
        // means the last thing said is not sitting in a page either.
        if let Ok(len) = std::fs::metadata(&self.scratch).map(|m| m.len())
            && len > 0
        {
            let _ = std::fs::write(&self.scratch, vec![0u8; len as usize]);
        }
        let _ = std::fs::remove_file(&self.scratch);
    }
}

fn peak_of(pcm: &[u8]) -> f32 {
    let mut peak = 0i32;
    for pair in pcm.chunks_exact(BYTES_PER_SAMPLE) {
        peak = peak.max((i16::from_le_bytes([pair[0], pair[1]]) as i32).abs());
    }
    peak as f32 / i16::MAX as f32
}

/// A 16 bit mono PCM wav, header written by hand.
pub fn write_wav(path: &Path, pcm: &[u8]) -> Result<()> {
    let mut f = std::fs::File::create(path)?;
    let data_len = pcm.len() as u32;

    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&(RATE * BYTES_PER_SAMPLE as u32).to_le_bytes())?;
    f.write_all(&(BYTES_PER_SAMPLE as u16).to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
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
        assert_eq!(u16::from_le_bytes([b[34], b[35]]), 16);
        assert_eq!(b.len(), 44 + (RATE as usize / 10) * 2);
    }

    #[test]
    fn an_empty_recording_still_writes_a_valid_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.wav");
        write_wav(&path, &[]).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 44);
    }

    #[test]
    fn silence_reads_as_quiet_and_a_tone_does_not() {
        assert_eq!(peak_of(&vec![0u8; 1000]), 0.0);

        let loud: Vec<u8> = (0..500)
            .flat_map(|i| (if i % 2 == 0 { 16000i16 } else { -16000 }).to_le_bytes())
            .collect();
        assert!(peak_of(&loud) > SPEECH_FLOOR);
    }

    /// The bug this whole module was rewritten for. The ring must never grow past the
    /// window, however long the consumer is away, or the consumer reads stale audio.
    #[test]
    fn the_ring_never_grows_past_its_window() {
        let cap = 100usize;
        let mut s = Shared::default();

        for _ in 0..50 {
            s.buf.extend(&[7u8; 32]);
            s.total += 32;
            while s.buf.len() > cap {
                s.buf.pop_front();
            }
        }
        assert_eq!(s.buf.len(), cap, "a slow consumer must not grow the buffer");
        assert_eq!(
            s.total, 1600,
            "but the running total still counts everything"
        );
    }
}
