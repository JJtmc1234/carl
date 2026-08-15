//! A panel that is nothing but the client, so what the client does is what you see.
//!
//! Deliberately has no state of its own beyond the last sequence, because that is the point being
//! demonstrated: everything about sockets, ordering, reconnection and gap recovery is in the
//! library, and a UI on top of it is a print statement.

use std::path::PathBuf;
use std::time::Duration;

use carl::panel::live::{Health, LivePanel, Update};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = PathBuf::from(std::env::args().nth(1).expect("usage: panel_watch <home>"));
    let socket = carl::panel::socket_path(&home);

    let (live, snapshot) = LivePanel::open(&socket)?;
    println!(
        "snapshot at seq {}: {} agents, {} tasks",
        snapshot.seq,
        snapshot.agents.len(),
        snapshot.tasks.len()
    );

    let mut live = live.quiet_after(Duration::from_millis(500));
    for _ in 0..40 {
        match live.next_update() {
            Update::Event(e) => {
                println!("  event  seq {:>2}  {:<11} {:?}", e.seq, e.kind, e.entity)
            }
            Update::Health(h) => println!("  link   {h:?}"),
            Update::Resynced(s) => println!("  RESYNCED to seq {} after a gap", s.seq),
            Update::Telemetry { at, diagnostics } => println!(
                "  sample at {at}, {} machine readings (no sequence: not a thing that happened)",
                diagnostics.len()
            ),
        }
        if live.health() == Health::Disconnected {
            println!("         (backend gone, still trying)");
        }
    }
    println!("last accepted sequence: {}", live.last_seq());
    Ok(())
}
