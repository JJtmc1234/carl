//! One exchange, start to finish.
//!
//! Two ways to ask, sharing every rule about what gets written down. `respond` waits for the
//! whole answer, which is right for a terminal. `stream` hands over the words as they are
//! written, which is right for a voice, because Claude takes five to twenty five seconds and
//! the first sentence is usually ready in under one.

use std::path::Path;

use anyhow::{Context, Result};
use carl::claude::{Answer, Flow, Pool, Runner, Turn};
use carl::{Area, Camera, ThreadId};

mod exchange;
use exchange::Exchange;

/// The full form, where what gets recorded and what gets sent can differ.
pub fn respond_full(
    runner: &Runner,
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    author: Option<String>,
) -> Result<Answer> {
    Exchange {
        home,
        thread,
        said,
        sent,
        author,
        extra: None,
    }
    .run(|p| {
        runner.ask(&Turn {
            session: &p.session,
            resume: p.resume,
            prompt: p.prompt,
            extra_system: Some(&p.all_in_system()),
            workdir: &p.workdir,
        })
    })
}

/// Handles one message, handing each piece of the answer over as it arrives.
///
/// `on_text` decides whether to keep going. Returning `Flow::Stop` abandons the rest, which
/// is what happens when Carl is talked over. `extra` carries the voice brief when the answer
/// is going to be spoken.
pub fn stream(
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    extra: Option<&str>,
    on_text: &mut dyn FnMut(&str) -> Flow,
) -> Result<Answer> {
    let runner = Runner::default();
    Exchange {
        home,
        thread,
        said,
        sent,
        author: None,
        extra,
    }
    .run(|p| {
        runner.ask_streaming(
            &Turn {
                session: &p.session,
                resume: p.resume,
                prompt: p.prompt,
                extra_system: Some(&p.all_in_system()),
                workdir: &p.workdir,
            },
            on_text,
        )
    })
}

/// Handles one message through a process that is already running.
///
/// The same recording rules and the same ordering, but the conversation is held open between
/// turns. Measured, a fresh process reaches its first token in 2.8 seconds and one that is
/// already running reaches it in 0.97, which in a spoken exchange is most of the wait.
///
/// The identity is not sent here, because the pool set it when it opened the process and a
/// system prompt cannot be changed afterwards. Everything that varies goes in front of the
/// question instead.
pub fn stream_in(
    pool: &mut Pool,
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    on_text: &mut dyn FnMut(&str) -> Flow,
) -> Result<Answer> {
    Exchange {
        home,
        thread,
        said,
        sent,
        author: None,
        // Static instructions belong to the process, which already has them.
        extra: None,
    }
    .run(|p| {
        pool.ask(
            thread,
            &p.session,
            p.resume,
            &p.question_with_context(),
            on_text,
        )
    })
}

/// Take a picture of the screen, then ask about it, through a running process.
pub fn look_in(
    pool: &mut Pool,
    home: &Path,
    thread: &ThreadId,
    question: &str,
    area: Area,
    on_text: &mut dyn FnMut(&str) -> Flow,
) -> Result<Answer> {
    let sent = shot(home, question, area)?;
    stream_in(pool, home, thread, question, Some(&sent), on_text)
}

/// Take a picture of the screen, then ask about it.
pub fn look(home: &Path, thread: &ThreadId, question: &str, area: Area) -> Result<Answer> {
    let sent = shot(home, question, area)?;
    respond_full(
        &Runner::default(),
        home,
        thread,
        question,
        Some(&sent),
        None,
    )
}

/// Takes the picture and writes the prompt that goes with it.
///
/// The image lands inside Claude Code's working directory, so the prompt can name it by a
/// short relative path and Claude reads it with its own file tools. No image encoding here.
fn shot(home: &Path, question: &str, area: Area) -> Result<String> {
    let workdir = home.join("workspace");
    let shot = workdir.join("screen.png");

    Camera::default()
        .capture(area, &shot)
        .context("could not take a picture of the screen")?;

    let (w, h) = carl::capture::png_size(&shot).unwrap_or((0, 0));

    // Told to look first and answer second. Asking the question first invites an answer from
    // memory before the image is ever opened.
    Ok(format!(
        "Read the image at ./screen.png ({w} by {h}). It is a picture of my screen taken \
just now. Look at it before answering, and answer from what is actually on screen rather \
than from what you expect to be there. If you cannot make something out, say so.\n\n{question}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use carl::Speaker;

    /// The question must be on disk before Claude is ever contacted, so a failure there
    /// cannot take it with it. Proven by pointing at a binary that does not exist.
    #[test]
    fn the_question_survives_a_total_failure_to_answer() {
        let home = tempfile::tempdir().unwrap();
        let thread = ThreadId::new("cli").unwrap();

        let missing = Runner::at("/nonexistent/definitely-not-claude");
        let result = respond_full(
            &missing,
            home.path(),
            &thread,
            "did you record this",
            None,
            None,
        );
        assert!(result.is_err(), "the ask should have failed");

        let entries = carl::log::read(home.path().join("conversations.jsonl")).unwrap();
        assert!(
            entries.iter().any(|e| e.text == "did you record this"),
            "the question must be recorded even when the answer never arrives: {entries:?}"
        );
        assert!(
            entries.iter().any(|e| e.speaker == Speaker::System),
            "the failure should be recorded too: {entries:?}"
        );
    }

    /// The streaming path shares the recording rules rather than reimplementing them, and
    /// this is what proves it did not drift.
    #[test]
    fn the_streaming_path_records_the_question_first_too() {
        let home = tempfile::tempdir().unwrap();
        let thread = ThreadId::new("cli").unwrap();

        let missing = Runner::at("/nonexistent/definitely-not-claude");
        let result = Exchange {
            home: home.path(),
            thread: &thread,
            said: "streamed question",
            sent: None,
            author: None,
            extra: None,
        }
        .run(|p| {
            missing.ask_streaming(
                &Turn {
                    session: &p.session,
                    resume: p.resume,
                    prompt: p.prompt,
                    extra_system: Some(&p.all_in_system()),
                    workdir: &p.workdir,
                },
                &mut |_| Flow::Continue,
            )
        });
        assert!(result.is_err());

        let entries = carl::log::read(home.path().join("conversations.jsonl")).unwrap();
        assert!(
            entries.iter().any(|e| e.text == "streamed question"),
            "{entries:?}"
        );
    }
}
