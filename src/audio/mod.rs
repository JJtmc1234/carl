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
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{Error, Result};

mod level;
mod listen;
mod wav;

pub use level::SPEECH_FLOOR;
pub use wav::write_wav;

/// Whisper wants 16 kHz mono. Feeding it anything else means resampling, so record it right.
pub const RATE: u32 = 16_000;
const BYTES_PER_SAMPLE: usize = 2;

/// How long the room keeps ringing after a speaker stops.
///
/// Not zero, because the sound card holds a buffer of its own and the walls hold the rest.
/// Unmuting the instant playback returns still catches the tail of Carl's own last word.
const SETTLE: Duration = Duration::from_millis(350);

#[derive(Default)]
pub(super) struct Shared {
    pub(super) buf: VecDeque<u8>,
    /// Every byte the microphone has ever produced. Lets a reader tell "nothing new yet"
    /// apart from "I fell behind and lost some".
    pub(super) total: u64,
}

pub struct Mic {
    pub(super) child: Child,
    pub(super) shared: Arc<Mutex<Shared>>,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) deaf: Arc<AtomicBool>,
    pub(super) reader: Option<JoinHandle<()>>,
    pub(super) scratch: PathBuf,
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
        let deaf = Arc::new(AtomicBool::new(false));

        let reader = {
            let shared = shared.clone();
            let stop = stop.clone();
            let deaf = deaf.clone();
            std::thread::spawn(move || {
                let mut chunk = vec![0u8; 4096];
                while !stop.load(Ordering::Relaxed) {
                    match pipe.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                            // Deaf still reads the pipe, and only then drops the bytes. A
                            // pipe nobody drains backs up, and the next thing heard would be
                            // however far behind real time the silence lasted.
                            if !deaf.load(Ordering::Relaxed) {
                                s.buf.extend(&chunk[..n]);
                            }
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
            deaf,
            reader: Some(reader),
            scratch: scratch_dir.join("listening.wav"),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

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
