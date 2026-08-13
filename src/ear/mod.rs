//! The loop. Listen, wake, answer, end, back to listening.
//!
//! Two states and four transitions, which is small enough to hold in your head, and that
//! matters because this is the code deciding what gets kept.
//!
//! ```text
//!   idle ──── nothing said to Carl ────> idle, and the audio is gone
//!   idle ──── "hey carl ..." ──────────> awake
//!  awake ──── anything else ───────────> answer it, stay awake
//!  awake ──── "end conversation" ──────> write a memory, back to idle
//! ```

use std::path::Path;

use anyhow::{Context, Result};
use carl::aec::Devices;
use carl::audio::Mic;
use carl::claude::{Pool, Runner};
use carl::{Heard, ThreadId, Tier, Voice, Whisper, heard};

mod mouth;
mod narrate;
mod reply;

use mouth::Mouth;

/// How much of the recent past Carl can see while idle. Comfortably longer than a wake word.
const WINDOW_SECS: f32 = 3.0;
/// How often the idle loop wakes up to look at that window.
const STEP_SECS: f32 = 0.6;
/// How long to wait for you to stop talking, and how long to wait before giving up.
///
/// Both are settable from the command line. There is no value that suits every person and
/// every room, and the last three bugs in this file were all a constant that fitted the
/// machine it was written on and nothing else.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// Seconds of quiet that mean you have finished speaking.
    pub hush: f32,
    /// Seconds before Carl stops waiting.
    pub cap: f32,
}

pub struct Ear {
    whisper: Whisper,
    mouth: Mouth,
    thread: ThreadId,
    devices: Devices,
}

/// Everything the loop needs that has to outlive one question.
///
/// The pool is the reason this exists. A held open `claude` process is what takes the wait
/// from about four seconds to about two, and it only helps if the same one is still there
/// when the next question arrives.
pub struct Running {
    pub pool: Pool,
    /// Rendered once at startup, because synthesising a noise on demand costs 0.29 seconds,
    /// which is most of the gap the noise exists to cover.
    pub filler: Option<carl::speech::Filler>,
}

impl Ear {
    pub fn new(thread: ThreadId, accurate: bool) -> Result<Self> {
        let devices = Devices::detect();
        let voice = Voice::found()
            .context("piper is not ready")?
            .to_sink(devices.sink());

        Ok(Self {
            whisper: Whisper::found()
                .context("whisper is not ready")?
                .accurate(accurate),
            mouth: Mouth::new(voice, devices.can_barge_in()),
            thread,
            devices,
        })
    }

    /// Runs until interrupted.
    pub fn run(&self, home: &Path, timing: Timing) -> Result<()> {
        // One process for the whole run. The voice is a single conversation, so it is opened
        // once and never closed, which is what removes the 0.8 seconds of startup and the cold
        // model from every single thing said out loud.
        let mut running = Running {
            pool: Pool::new(
                Runner::default(),
                home.join("workspace"),
                carl::brief::spoken(),
            ),
            filler: None,
        };

        // Audio scratch lives in RAM. Nothing recorded ever reaches the disk, whether it is
        // kept or not, so the discard below is a real one.
        let mic = Mic::open(
            WINDOW_SECS,
            Path::new("/dev/shm/carl"),
            self.devices.source(),
        )?;
        let mut awake = false;

        if self.devices.can_barge_in() {
            eprintln!("echo cancelled audio. you can talk over him.");
        } else {
            eprintln!(
                "no echo canceller, so Carl goes deaf while he speaks and cannot be \
                 interrupted. Start it with: pipewire -c etc/carl-aec.conf"
            );
        }

        // Measure the room before listening to it. A fixed threshold cannot fit a laptop
        // fan and a quiet study at the same time, and getting it wrong in the loud
        // direction means Carl records until his cap every single time.
        eprint!("measuring the room... ");
        let hush = mic.calibrate(1.5);
        eprintln!("quiet is below {hush:.3}");

        // Rendered after the room is measured rather than before, so the first thing that
        // happens on startup is still listening. A filler that fails to render costs nothing:
        // Carl without one is the Carl that existed before it.
        let filler = carl::speech::Filler::prepare(self.mouth.voice(), Path::new("/dev/shm/carl"));
        if filler.is_ready() {
            running.filler = Some(filler);
        }

        eprintln!("listening. say \"hey carl\" to start, \"end conversation\" to finish.");

        loop {
            if !awake {
                mic.wait(STEP_SECS);

                // Silence is most of a room's day. Running whisper over it costs real time
                // for a guaranteed empty answer, so the level check comes first.
                let level = mic.loudness();
                if level < hush {
                    continue;
                }

                let started = std::time::Instant::now();
                let text = self.whisper.transcribe(Tier::Wake, mic.snapshot()?)?;

                // Printed every pass, not hidden behind a flag. When a wake word does not
                // land you need to see what the model actually wrote, and needing to restart
                // with a flag to find out is a step too many when the thing is already
                // failing in front of you.
                if !text.is_empty() {
                    eprintln!(
                        "  [{:.1}s, level {:.2}] heard: {text:?}",
                        started.elapsed().as_secs_f32(),
                        level
                    );
                }

                match heard::interpret(&text, false) {
                    // Not for Carl. Nothing is transcribed further, nothing is written down,
                    // and the window rolls the audio out of memory on its own.
                    Heard::Nothing => continue,
                    Heard::Wake { question } => {
                        awake = true;
                        eprintln!("  awake.");
                        mic.forget();
                        match question {
                            // Asked on the same breath, so answer it rather than making them
                            // say it twice.
                            Some(q) => self.answer(&mut running, &mic, home, &q, hush)?,
                            None => {
                                self.mouth.say(&mic, "Yes?", hush)?;
                            }
                        }
                    }
                    // interpret only returns these when already listening.
                    Heard::Say(_) | Heard::End => continue,
                }
                continue;
            }

            // Awake. Capture a whole sentence rather than a window, because a sentence can
            // easily run longer than one.
            eprint!("  listening... ");
            let wav = mic.utterance(timing.hush, timing.cap, hush)?;
            let text = self.whisper.transcribe(Tier::Talk, wav)?;
            eprintln!("heard: {text:?}");

            match heard::interpret(&text, true) {
                Heard::End => {
                    self.remember(&mut running, home)?;
                    self.mouth.say(&mic, "Alright. I'll remember that.", hush)?;
                    awake = false;
                    eprintln!("back to listening.");
                }
                Heard::Say(q) => self.answer(&mut running, &mic, home, &q, hush)?,
                // Nothing usually means the utterance was noise, not speech. Staying awake
                // rather than dropping out avoids the loop where a cough ends the
                // conversation and you have to wake him again.
                Heard::Nothing => continue,
                Heard::Wake { question } => {
                    if let Some(q) = question {
                        self.answer(&mut running, &mic, home, &q, hush)?;
                    }
                }
            }
        }
    }
}
