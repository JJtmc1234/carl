//! The Command Panel v1 contract, driven through the same client the panel's reader thread uses.
//!
//! Not a substitute for looking at the window. It proves everything between the journal and the
//! panel's data source, which is where the ordering promises live, and it can be run on a
//! temporary home by anybody at any time. What it cannot prove is pixels.

use std::path::PathBuf;
use std::time::Duration;

use carl::panel::live::{Health, LivePanel, Update};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = PathBuf::from(std::env::args().nth(1).expect("usage: v1_endtoend <home>"));
    let socket = carl::panel::socket_path(&home);

    let (live, snapshot) = LivePanel::open(&socket)?;
    let mut live = live.quiet_after(Duration::from_millis(400));
    println!(
        "snapshot seq {}  agents {}  projects {}  diagnostics {}",
        snapshot.seq,
        snapshot.agents.len(),
        snapshot.projects.len(),
        snapshot.diagnostics.len()
    );

    let mut events = Vec::new();
    let mut telemetry = 0;
    let mut healths = Vec::new();
    let start = std::time::Instant::now();

    while start.elapsed() < Duration::from_secs(40) {
        match live.next_update() {
            Update::Event(e) => {
                println!(
                    "  event   seq {:>2}  {:<11} last_seq now {}",
                    e.seq,
                    e.kind,
                    live.last_seq()
                );
                events.push(e.seq);
            }
            Update::Telemetry { at, .. } => {
                telemetry += 1;
                if telemetry <= 2 || telemetry % 10 == 0 {
                    println!("  sample  at {at}  last_seq still {}", live.last_seq());
                }
            }
            Update::Health(h) => {
                println!("  link    {h:?}");
                healths.push(h);
            }
            Update::Resynced(s) => println!("  RESYNC  to seq {}", s.seq),
            Update::Asked(r) => println!("  ASKED   {} {}", r.tool, r.detail),
            Update::Answered { question, verdict } => {
                println!("  SETTLED {question} {}", verdict.word())
            }
        }
        if events.len() >= 2 && healths.contains(&Health::Connected) && telemetry > 2 {
            break;
        }
    }

    println!("\nevents seen: {events:?}");
    println!("telemetry frames: {telemetry}");
    println!("link transitions: {healths:?}");
    println!("final last_seq: {}", live.last_seq());
    let mut sorted = events.clone();
    sorted.sort_unstable();
    sorted.dedup();
    println!("no duplicates and in order: {}", sorted == events);
    Ok(())
}
