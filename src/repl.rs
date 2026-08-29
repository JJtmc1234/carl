//! Talking to Carl by typing, without starting him again for every line.
//!
//! `carl ask` runs a whole process per question. That is right for one question inside a
//! script and wrong for a conversation, because every line pays about 0.8 seconds of startup
//! and then a cold model, measured at 2.8 seconds to a first word against 0.97 for one
//! already running.
//!
//! So this holds the conversation open, which is the same thing the voice does and the same
//! thing every Slack thread does. The only difference is where the words come from.
//!
//! Deliberately not a fancy terminal. No cursor control, no colours beyond one dim prompt, no
//! alternate screen. It has to work over ssh, inside a pipe, and in whatever terminal is
//! actually being used, and the way to be sure of that is not to try anything clever.

use std::io::{BufRead, Write};
use std::path::Path;

use anyhow::Result;
use carl::claude::{Flow, Pool};
use carl::turn;

/// Words that end the conversation, on their own on a line.
///
/// `exit` and `quit` because everybody tries them. Control D also works and is the real
/// answer, but nobody should have to know that to get out of something they just started.
const LEAVE: &[&str] = &["exit", "quit", "bye", ":q"];

pub fn run(home: &Path, thread: &str) -> Result<()> {
    let thread = carl::ThreadId::new(thread)?;

    // One process for the whole session, opened on the first question rather than now, so
    // starting the terminal and never typing anything costs nothing.
    // JJ's own terminal, so it reads the `jj` permits rather than the defaults. It used to take
    // `Runner::default()`, which is why nothing JJ wrote in permissions.json ever reached here.
    let mut pool = Pool::new(
        carl::turn::runner_for(home, carl::claude::permits::Surface::Jj)?,
        home.join("workspace"),
        carl::brief::IDENTITY,
    );

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();

    eprintln!(
        "talking to carl in thread {thread}. control d or \"exit\" to finish.\n\
         start a line with \"look\" to have him look at your screen."
    );

    loop {
        write!(out, "\n> ")?;
        out.flush()?;

        let mut line = String::new();
        // Zero bytes means the input ended, which is control d or the end of a pipe. Both mean
        // the same thing and both are an ordinary way to stop.
        if stdin.lock().read_line(&mut line)? == 0 {
            eprintln!();
            break;
        }

        let said = line.trim();
        if said.is_empty() {
            continue;
        }
        if LEAVE.contains(&said.to_lowercase().as_str()) {
            break;
        }

        // Judged the same way the voice judges it, so "should I put it here" looks and "what
        // should I research" does not. Typing has one advantage over speaking: you can say
        // "look" and mean it, so that is honoured outright.
        let (question, look) = match said.strip_prefix("look ") {
            Some(rest) => (rest.trim(), true),
            None if said.eq_ignore_ascii_case("look") => ("What is on my screen?", true),
            None => (said, carl::needs_screen(said)),
        };
        if look {
            eprintln!("(looking at your screen, which flashes it)");
        }

        // Printed as it arrives, and only the part anybody should see. A [remember] line is
        // Carl writing something down, not talking, so it is stripped before the terminal ever
        // gets it rather than after.
        let mut shown = 0usize;
        let mut whole = String::new();

        let asking = turn::Asking {
            pool: &mut pool,
            home,
            thread: &thread,
            said: question,
            sent: None,
            said_by: Some(carl::brief::OWNER),
        };

        let mut on_text = |chunk: carl::Say<'_>| {
            // Notes are printed and never kept. Only the words are the answer, so only the
            // words go into `whole`, which is what gets remembered and reprinted.
            let chunk = match chunk {
                carl::Say::Words(t) => t,
                carl::Say::Thinking(t) => {
                    let _ = write!(out, "\x1b[2m{t}\x1b[0m");
                    let _ = out.flush();
                    return Flow::Continue;
                }
                carl::Say::Doing { tool, detail } => {
                    let _ = write!(
                        out,
                        "\x1b[2m{}\x1b[0m",
                        carl::claude::doing_line(tool, detail)
                    );
                    let _ = out.flush();
                    return Flow::Continue;
                }
                carl::Say::Refused { tool, why } => {
                    let _ = write!(out, "{}", carl::claude::refusal_line(tool, why));
                    let _ = out.flush();
                    return Flow::Continue;
                }
            };
            whole.push_str(chunk);
            let visible = carl::remember::split(&whole).text;

            // Only ever appends, and only on a character boundary. Stripping a note can
            // shorten the visible text, nothing already printed can be taken back, and
            // `shown` is a byte offset into an older version of the string.
            if let Some(fresh) = visible.get(shown..)
                && !fresh.is_empty()
            {
                let _ = out.write_all(fresh.as_bytes());
                let _ = out.flush();
                shown = visible.len();
            }
            Flow::Continue
        };

        // Nothing interrupts a typed answer. There is no microphone, and the next line cannot
        // be typed until this one is done.
        let mut waiting = || Flow::Continue;

        let answer = if look {
            turn::look_in(asking, carl::Area::Screen, &mut on_text, &mut waiting)
        } else {
            turn::stream_in(asking, &mut on_text, &mut waiting)
        };

        writeln!(out)?;
        match answer {
            Ok(a) if a.text.trim().is_empty() => {
                eprintln!("(nothing came back, which is probably a bug)");
            }
            Ok(_) => {}
            // Printed and carried on. A failed question in a conversation is one bad turn, not
            // a reason to throw away the session and everything in it.
            Err(e) => eprintln!("failed: {e:#}"),
        }
    }

    eprintln!("bye.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everybody tries these before they try control d, and being unable to leave something
    /// you just started is a bad first minute.
    #[test]
    fn the_obvious_ways_to_leave_all_work() {
        for word in ["exit", "quit", "EXIT", "Quit", "bye", ":q"] {
            assert!(
                LEAVE.contains(&word.to_lowercase().as_str()),
                "{word} should end the conversation"
            );
        }
    }

    /// A question that happens to contain the word must not end the session.
    #[test]
    fn a_sentence_containing_one_is_not_one() {
        for said in ["how do I exit vim", "quit messing about", "say bye to it"] {
            assert!(
                !LEAVE.contains(&said.to_lowercase().as_str()),
                "{said} is a question, not a goodbye"
            );
        }
    }
}
