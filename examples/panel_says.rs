//! What the panel actually receives while Carl answers, one line per frame.
//!
//! Written because "the panel shows chain of thought" was reported as done twice and was not
//! visible either time. The first time the work was on a branch nobody had merged. The second
//! time the window on screen had been started before the binary was rebuilt, so it was serving
//! the old one. Both are deployment, and neither is something the tests can catch: every test
//! passed while the thing on the screen did nothing.
//!
//! So this asks the running backend a real question and prints the kind of every frame that
//! comes back. If `thinking` and `doing` lines appear here, the backend is producing them and
//! anything still missing is the window in front of you rather than the code.
//!
//! Usage: `cargo run --example panel_says -- <home> "your question"`

use std::path::PathBuf;

use carl::panel::client::{Heard, PanelClient};
use carl::panel::command::PanelCommand;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let home = PathBuf::from(args.next().expect("usage: panel_says <home> [question]"));
    let asked: String = match args.collect::<Vec<_>>().join(" ") {
        empty if empty.trim().is_empty() => {
            "In one short sentence, what is the chain of command here?".to_string()
        }
        given => given,
    };

    let socket = carl::panel::socket_path(&home);
    println!("asking through {}", socket.display());
    println!("  {asked}\n");

    let mut client = PanelClient::connect(&socket)?;
    // No read timeout. The gap between frames is however long Carl thinks for, and a timeout
    // here turns "still working" into an error, which is what happened the first time this ran:
    // it printed a real tool call and then died with WouldBlock while he was mid answer.
    // Bound the whole thing from outside with `timeout` instead.
    client.read_timeout(None)?;

    let (mut words, mut thinking, mut doing) = (0usize, 0usize, 0usize);
    let done = client.command_streaming(
        PanelCommand::Say { text: asked },
        &mut |heard| match heard {
            Heard::Words(t) => {
                words += t.len();
                print!("{t}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
            }
            Heard::Thinking(t) => {
                thinking += t.len();
                eprintln!("[thinking] {}", first_line(t));
            }
            Heard::Doing { tool, detail } => {
                doing += 1;
                eprintln!("[doing]    {tool} {}", first_line(detail));
            }
        },
    )?;

    // Not printed again when it already streamed. `speak` streams every word and then returns
    // the finished answer in `Done`, which is right: a caller that missed the stream still gets
    // the answer. Printing both is this example's mistake, and it made the backend look like it
    // was sending everything twice.
    if words == 0 {
        println!("{}", done.what);
    }
    println!();
    println!("frames: {words} bytes of answer, {thinking} bytes of reasoning, {doing} tool calls");
    if thinking == 0 && doing == 0 {
        println!(
            "\nNothing but words came back. Either this turn used no tools and produced no \
             reasoning, or the backend serving that socket predates the typed channel. Check \
             which binary is running before concluding it is broken."
        );
    }
    Ok(())
}

/// One line, short, so a long thought does not take the terminal with it.
fn first_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(100) {
        Some((at, _)) => format!("{}...", &flat[..at]),
        None => flat,
    }
}
