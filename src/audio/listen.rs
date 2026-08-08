//! What a caller does with a running microphone: look at it, measure it, record from it,
//! and switch it off while Carl talks.
//!
//! Split from the module that owns the `arecord` child and the reader thread. Starting a
//! microphone and listening to one are different jobs, and only the first has anything to
//! say about processes and pipes.

use std::path::Path;
use std::time::{Duration, Instant};

use super::{BYTES_PER_SAMPLE, Mic, RATE, SETTLE};
use crate::Result;
use crate::audio::level::{SPEECH_FLOOR, rms_of};
use crate::audio::wav::write_wav;
use std::sync::atomic::Ordering;

impl Mic {
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

    /// Loudness as RMS, not peak.
    ///
    /// Peak is the wrong measure for "is anyone talking". One keyboard click sends peak to
    /// 0.24 in a silent room, so a peak based silence test never fires and a recording runs
    /// to its cap. RMS averages over the window and tracks how loud it actually is.
    pub fn loudness(&self) -> f32 {
        rms_of(&self.window().0)
    }

    /// Loudness of just the last moment, rather than the whole window.
    ///
    /// Deciding whether someone has started talking over Carl cannot use the full three
    /// second window. Averaging half a second of speech against two and a half of silence
    /// hides it, and by the time the window fills enough to notice, the interruption is over
    /// and Carl has kept talking through all of it.
    pub fn recent(&self, secs: f32) -> f32 {
        let want = (RATE as f32 * secs) as usize * BYTES_PER_SAMPLE;
        let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
        let take = want.min(s.buf.len());
        let tail: Vec<u8> = s.buf.iter().rev().take(take).rev().copied().collect();
        drop(s);
        rms_of(&tail)
    }

    /// Measures the room so the silence threshold fits it rather than a guess.
    ///
    /// Returns the level below which this room counts as quiet. Rooms differ by more than
    /// any constant can cover: a laptop fan, an air conditioner and a quiet study are not
    /// the same number, and picking one wrong either records forever or cuts you off.
    pub fn calibrate(&self, secs: f32) -> f32 {
        self.wait(secs);
        let room = rms_of(&self.window().0);
        // Comfortably above the room but far below speech. Speech typically sits ten times
        // over its own background, so tripling leaves room for both.
        (room * 3.0).max(SPEECH_FLOOR)
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

    /// Runs something with the microphone deaf, and forgets everything it missed.
    ///
    /// A speaker and a microphone in one room are a loop. Carl says an answer, the mic hears
    /// it, whisper transcribes it, and Carl answers his own answer. Nothing downstream can
    /// tell his voice from yours, so the only place to break the loop is here.
    ///
    /// This makes him half duplex: he cannot hear you while he talks. That is a real
    /// limitation and the honest one to ship, because the alternative is a Carl who
    /// interrupts himself. Echo cancellation in the audio server is what lifts it, and that
    /// belongs in the audio server rather than in here.
    pub fn deaf_while<T>(&self, f: impl FnOnce() -> T) -> T {
        // A guard rather than two stores, so a panic inside `f` cannot leave Carl permanently
        // deaf with no sign of why.
        struct Deafness<'a>(&'a Mic);
        impl Drop for Deafness<'_> {
            fn drop(&mut self) {
                std::thread::sleep(SETTLE);
                self.0.deaf.store(false, Ordering::Relaxed);
                self.0.forget();
            }
        }

        self.deaf.store(true, Ordering::Relaxed);
        let _guard = Deafness(self);
        f()
    }

    /// True while Carl is speaking and deliberately not listening.
    pub fn is_deaf(&self) -> bool {
        self.deaf.load(Ordering::Relaxed)
    }

    /// Records one whole thing you said, from now until you stop talking.
    ///
    /// The window is right for spotting a wake word and wrong for a sentence, which can run
    /// longer than the window and would lose its front. This grows instead.
    pub fn utterance(&self, hush_secs: f32, cap_secs: f32, hush_level: f32) -> Result<&Path> {
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

            if pcm.len() > hush * 2 && rms_of(&pcm[pcm.len() - hush..]) < hush_level {
                break;
            }
        }

        write_wav(&self.scratch, &pcm)?;
        Ok(&self.scratch)
    }
}
