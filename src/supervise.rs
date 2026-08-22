//! The loop around one supervisor pass.
//!
//! Everything that decides anything is in the library. This is a clock, a print, and the
//! handling of the two things that can be wrong with a home before a supervisor can do
//! anything at all.
//!
//! **The loop lives here rather than in the supervisor** so the supervisor has no clock of its
//! own and can be tested by handing it one. It is also the piece a systemd unit will eventually
//! replace the terminal for: `Restart=always` around this, and the supervisor gets the same
//! treatment it gives its agents.
//!
//! **No signal handling, and that is deliberate rather than missing.** A supervisor stopped with
//! SIGTERM leaves its lock file behind, and the lock carries the start time of the process that
//! took it, so the next supervisor recognises it as stale and takes it. Catching the signal to
//! tidy up would be a second mechanism for something that already cannot go wrong, and it would
//! still not cover being killed outright.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use carl::army::personnel::Personnel;
use carl::army::runtime::{Outcome, Supervisor};

/// Runs until interrupted, or once.
pub fn run(home: &Path, program: &str, every: u64, once: bool) -> Result<()> {
    let mut supervisor = Supervisor::take(home, program)?;
    println!("supervising the army in {}", home.display());

    // What was printed last time. A supervisor that says the same four lines every few seconds
    // is a supervisor nobody reads, and the whole value of the output is that a change in it
    // means something changed.
    let mut said: Option<Vec<(String, Outcome)>> = None;

    loop {
        match Personnel::open(home) {
            // Not fatal. A folder that will not load is a thing to fix, and a supervisor that
            // exited over it would also stop every agent that was perfectly fine.
            Err(e) => eprintln!("  the agent folders will not load: {e}"),
            Ok(people) if people.is_empty() => {
                eprintln!("  no army has been founded in this home, so there is nobody to run")
            }
            Ok(people) => {
                let at = now();

                // Before the tick, so a tick never starts an agent this pass is about to put
                // down for the night.
                for (name, clocked) in supervisor.keep_hours(&people, at)? {
                    println!("  {name:8} {clocked}");
                    // The army looks different now, so whatever was printed last no longer
                    // describes it and the next tick should say so in full.
                    said = None;
                }

                let tick = supervisor.tick(&people, at)?;
                if said.as_ref() != Some(&tick.what) {
                    for line in tick.lines() {
                        println!("{line}");
                    }
                    said = Some(tick.what);
                }
            }
        }

        if once {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(every.max(1)));
    }
}

/// Unix seconds.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
