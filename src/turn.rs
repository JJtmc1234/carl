//! One exchange, start to finish.
//!
//! The ordering here is the whole point, and it is deliberate.
//!
//! 1. Record what the person said, before anything can fail.
//! 2. Mint or look up the session for this thread.
//! 3. Ask Claude Code, resuming that session.
//! 4. Record the answer, or record the failure.
//!
//! Recording first means a crash loses the answer, never the question. That is the
//! recoverable direction: an unanswered question can be asked again, and a question nobody
//! wrote down is gone. Same rule the AOS event log follows.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use carl::claude::{Answer, Runner, Turn};
use carl::{Area, Camera, Log, Memory, Registry, Speaker, ThreadId};

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// Handles one message and returns Carl's answer.
///
/// `author` names who spoke, for Slack. `None` means the local terminal.
pub fn respond(
    home: &Path,
    thread: &ThreadId,
    said: &str,
    author: Option<String>,
) -> Result<Answer> {
    respond_with(&Runner::default(), home, thread, said, author)
}

/// Same, with the runner injected.
///
/// Split out so a test can point at a binary that does not exist without touching `PATH`,
/// which is process wide and would break whatever else is running at the same time.
pub fn respond_with(
    runner: &Runner,
    home: &Path,
    thread: &ThreadId,
    said: &str,
    author: Option<String>,
) -> Result<Answer> {
    respond_full(runner, home, thread, said, None, author)
}

/// The full form, where what gets recorded and what gets sent can differ.
///
/// `sent` is what Claude actually receives. When a screenshot is involved it carries the
/// image path, which would be noise in the record. The record keeps the human sentence.
pub fn respond_full(
    runner: &Runner,
    home: &Path,
    thread: &ThreadId,
    said: &str,
    sent: Option<&str>,
    author: Option<String>,
) -> Result<Answer> {
    let mut log = Log::open(home.join("conversations.jsonl"))
        .context("cannot open the conversation record")?;

    // Step 1. Before anything else, and before anything can go wrong.
    log.append(now(), thread.clone(), Speaker::Human, said, author)?;

    let mut registry = Registry::open(home.join("threads.json"))?;
    let (session, is_new) = registry.session_for(thread, now())?;

    let memory = Memory::open(home.join("memory"))?;
    let extra_system = memory.assemble()?;

    // Claude Code runs with this as its working directory, so anything it writes lands
    // somewhere predictable rather than wherever carl happened to be started from.
    let workdir = home.join("workspace");

    let outcome = runner.ask(&Turn {
        session: &session,
        // A brand new thread has nothing to resume. Getting this wrong is the difference
        // between continuing a conversation and starting a second one silently.
        resume: !is_new,
        prompt: sent.unwrap_or(said),
        extra_system: extra_system.as_deref(),
        workdir: &workdir,
    });

    match outcome {
        Ok(answer) => {
            // A session id we did not ask for means the next turn would resume the wrong
            // conversation. Worth recording rather than shrugging at.
            if let Some(actual) = &answer.session_id
                && actual != session.as_str()
            {
                log.append(
                    now(),
                    thread.clone(),
                    Speaker::System,
                    format!(
                        "claude answered on session {actual} but was asked for {session}, \
                         so this thread may have split"
                    ),
                    None,
                )?;
            }

            log.append(now(), thread.clone(), Speaker::Carl, &answer.text, None)?;
            registry.record_turn(thread)?;
            Ok(answer)
        }
        Err(e) => {
            // The failure goes in the record too. A gap with no explanation is the hardest
            // kind of thing to investigate later.
            log.append(
                now(),
                thread.clone(),
                Speaker::System,
                format!("no answer: {e}"),
                None,
            )?;
            Err(e.into())
        }
    }
}

/// Take a picture of the screen, then ask about it.
///
/// The image lands inside Claude Code's working directory, so the prompt can name it by a
/// short relative path and Claude reads it with its own file tools. No image encoding here.
pub fn look(home: &Path, thread: &ThreadId, question: &str, area: Area) -> Result<Answer> {
    let workdir = home.join("workspace");
    let shot = workdir.join("screen.png");

    Camera::default()
        .capture(area, &shot)
        .context("could not take a picture of the screen")?;

    let (w, h) = carl::capture::png_size(&shot).unwrap_or((0, 0));

    // Told to look first and answer second. Asking the question first invites an answer from
    // memory before the image is ever opened.
    let sent = format!(
        "Read the image at ./screen.png ({w} by {h}). It is a picture of my screen taken \
just now. Look at it before answering, and answer from what is actually on screen rather \
than from what you expect to be there. If you cannot make something out, say so.\n\n{question}"
    );

    respond_full(
        &Runner::default(),
        home,
        thread,
        question,
        Some(&sent),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The question must be on disk before Claude is ever contacted, so a failure there
    /// cannot take it with it. Proven by pointing at a binary that does not exist.
    #[test]
    fn the_question_survives_a_total_failure_to_answer() {
        let home = tempfile::tempdir().unwrap();
        let thread = ThreadId::new("cli").unwrap();

        let missing = Runner::at("/nonexistent/definitely-not-claude");
        let result = respond_with(&missing, home.path(), &thread, "did you record this", None);
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
}
