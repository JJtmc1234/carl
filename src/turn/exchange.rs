//! The ordering, in one place.
//!
//! 1. Record what the person said, before anything can fail.
//! 2. Mint or look up the session for this thread.
//! 3. Ask Claude Code, resuming that session.
//! 4. Record the answer, or record the failure.
//!
//! Recording first means a crash loses the answer, never the question. That is the
//! recoverable direction: an unanswered question can be asked again, and a question nobody
//! wrote down is gone. Same rule the AOS event log follows.
//!
//! There is one copy of this because there are two ways to ask. Waiting for the whole answer
//! and reading it as it is written differ only in how the words arrive, and duplicating the
//! bookkeeping would mean the streaming path quietly drifting out of step on the one thing
//! that actually matters.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use carl::claude::{Answer, Turn};
use carl::{Log, Memory, Registry, Speaker, ThreadId};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

pub struct Exchange<'a> {
    pub home: &'a Path,
    pub thread: &'a ThreadId,
    /// What the person actually said. This is what goes in the record.
    pub said: &'a str,
    /// What Claude receives, when it differs. A screenshot question carries the image path,
    /// which would be noise in the record.
    pub sent: Option<&'a str>,
    pub author: Option<String>,
    /// Standing instructions for this way of asking, appended after the memory notes.
    ///
    /// Carries the voice brief on spoken turns and nothing at all on typed ones, because the
    /// rule that makes a good spoken answer makes a uselessly thin written one.
    pub extra: Option<&'a str>,
}

impl Exchange<'_> {
    /// Records the question, prepares the turn, runs `ask`, records the outcome.
    pub fn run(self, ask: impl FnOnce(&Turn<'_>) -> carl::Result<Answer>) -> Result<Answer> {
        let mut log = Log::open(self.home.join("conversations.jsonl"))
            .context("cannot open the conversation record")?;

        // Step 1. Before anything else, and before anything can go wrong.
        log.append(
            now(),
            self.thread.clone(),
            Speaker::Human,
            self.said,
            self.author,
        )?;

        let mut registry = Registry::open(self.home.join("threads.json"))?;
        let (session, is_new) = registry.session_for(self.thread, now())?;

        let memory = Memory::open(self.home.join("memory"))?;
        let extra_system = match (memory.assemble()?, self.extra) {
            (Some(notes), Some(extra)) => Some(format!("{notes}\n\n{extra}")),
            (Some(notes), None) => Some(notes),
            (None, Some(extra)) => Some(extra.to_string()),
            (None, None) => None,
        };

        // Claude Code runs with this as its working directory, so anything it writes lands
        // somewhere predictable rather than wherever carl happened to be started from.
        let workdir = self.home.join("workspace");

        let outcome = ask(&Turn {
            session: &session,
            // A brand new thread has nothing to resume. Getting this wrong is the difference
            // between continuing a conversation and starting a second one silently.
            resume: !is_new,
            prompt: self.sent.unwrap_or(self.said),
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
                        self.thread.clone(),
                        Speaker::System,
                        format!(
                            "claude answered on session {actual} but was asked for {session}, \
                             so this thread may have split"
                        ),
                        None,
                    )?;
                }

                log.append(
                    now(),
                    self.thread.clone(),
                    Speaker::Carl,
                    &answer.text,
                    None,
                )?;

                // Recorded, because the record would otherwise claim Carl said things nobody
                // ever heard. Claude's own session still holds the full answer, so the two
                // genuinely differ from here on and the difference has to be written down.
                if answer.interrupted {
                    log.append(
                        now(),
                        self.thread.clone(),
                        Speaker::System,
                        "interrupted, so only the text above was said out loud",
                        None,
                    )?;
                }

                registry.record_turn(self.thread)?;
                Ok(answer)
            }
            Err(e) => {
                // The failure goes in the record too. A gap with no explanation is the hardest
                // kind of thing to investigate later.
                log.append(
                    now(),
                    self.thread.clone(),
                    Speaker::System,
                    format!("no answer: {e}"),
                    None,
                )?;
                Err(e.into())
            }
        }
    }
}
