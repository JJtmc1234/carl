//! Asks for snapshots as fast as a render loop would, and reports what it cost.
//!
//! One connection, so the measurement is of the backend rather than of starting clients. Pings
//! are the control: they cross the same socket and do no provider work at all, so the difference
//! between the two runs is what a snapshot actually costs.

use std::path::PathBuf;
use std::time::Instant;

fn forks() -> u64 {
    std::fs::read_to_string("/proc/stat")
        .unwrap_or_default()
        .lines()
        .find_map(|l| l.strip_prefix("processes ")?.trim().parse().ok())
        .unwrap_or(0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(args.next().expect("usage: panel_hammer <home> [n]"));
    let n: u32 = args.next().unwrap_or_else(|| "300".into()).parse()?;
    let socket = carl::panel::socket_path(&home);

    let mut client = carl::panel::client::PanelClient::connect(&socket)?;
    let before = forks();
    let began = Instant::now();
    for _ in 0..n {
        client.ping()?;
    }
    let ping_forks = forks() - before;
    let ping_time = began.elapsed();

    let mut client = carl::panel::client::PanelClient::connect(&socket)?;
    let before = forks();
    let began = Instant::now();
    let mut last = 0;
    for _ in 0..n {
        last = client.snapshot()?.diagnostics.len();
    }
    let snap_forks = forks() - before;
    let snap_time = began.elapsed();

    println!(
        "{n} pings     {:>6.0}ms, {ping_forks:>4} forks system wide",
        ping_time.as_secs_f64() * 1000.0
    );
    println!(
        "{n} snapshots {:>6.0}ms, {snap_forks:>4} forks system wide, {last} diagnostics each",
        snap_time.as_secs_f64() * 1000.0
    );
    println!(
        "difference: {} forks for {n} snapshots",
        snap_forks.saturating_sub(ping_forks)
    );
    println!(
        "the old army() forked systemctl 3 times per call, so {n} snapshots would have been {} on their own",
        n * 3
    );
    Ok(())
}
