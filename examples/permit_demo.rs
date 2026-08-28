//! Watches for permission questions and answers them, the way JJ clicking would.
//!
//! For proving the loop against a real `claude` process without a window open. It is an example
//! rather than a subcommand on purpose: a program whose whole job is to say yes to tool calls is
//! not something that should be one typo away on the real binary.
//!
//! ```text
//!   cargo run --example permit_demo -- <home> allow
//!   cargo run --example permit_demo -- <home> deny
//! ```
//!
//! Prints every question it sees either way, so the run is readable afterwards.

use std::path::PathBuf;

use carl::panel::permission::Verdict;
use carl::panel::{Incoming, PanelClient, socket_path};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(args.next().ok_or("usage: permit_demo <home> allow|deny")?);
    let verdict = match args.next().as_deref() {
        Some("allow") => Verdict::Allow,
        Some("deny") | None => Verdict::Deny,
        Some(other) => return Err(format!("allow or deny, not {other:?}").into()),
    };

    let at = socket_path(&home);
    println!("watching {}, answering {}", at.display(), verdict.word());

    let mut events = PanelClient::connect(&at)?.subscribe(0)?;
    loop {
        match events.recv()? {
            Incoming::Asked(request) => {
                println!(
                    "  asked  {} by {}: {}",
                    request.tool, request.surface, request.detail
                );
                // A fresh connection, because this one is carrying the stream.
                PanelClient::connect(&at)?.answer(&request.id, verdict)?;
                println!("  said   {}", verdict.word());
            }
            Incoming::Answered { question, verdict } => {
                println!("  settled {question} {}", verdict.word());
            }
            _ => continue,
        }
    }
}
