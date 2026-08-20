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

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::claude::Answer;
use crate::{Log, Memory, Registry, SessionId, Speaker, ThreadId};
use anyhow::{Context, Result};

/// Drops a note, named either by its filename or by what it said.
///
/// Both, because Carl sees the filenames in his own memory block and will sometimes quote
/// one, and will otherwise write out the fact in his own words. Accepting only one of those
/// would mean `[forget]` silently doing nothing half the time, which is the worst outcome
/// available: he would believe the wrong fact was gone and keep being told it.
fn forget_note(memory: &Memory, named: &str) -> anyhow::Result<bool> {
    let bare = named.trim().trim_end_matches(".md");

    // Tried as a filename only if it could be one. A fact has spaces in it, and handing that
    // to Memory is a validation error rather than a miss, which would turn "no such note"
    // into a failure and hide the real outcome.
    let could_be_a_filename = !bare.is_empty()
        && bare
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');

    if could_be_a_filename && memory.forget(bare)? {
        return Ok(true);
    }

    // Otherwise as a fact, put through the same naming rule that wrote it.
    let slug = crate::remember::note_name(bare);
    if !slug.is_empty() && memory.forget(&slug)? {
        return Ok(true);
    }
    Ok(false)
}

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
    /// Extra instructions for this way of asking, on top of the identity every turn gets.
    ///
    /// Carries the spoken brief on voice turns and nothing on typed ones, because the rule
    /// that makes a good spoken answer makes a uselessly thin written one.
    pub extra: Option<&'a str>,
}

/// One prepared turn, split by how long each piece stays true.
///
/// The split exists because a held open `claude` process fixes its system prompt when it
/// starts and cannot be told anything new there afterwards. Who Carl is never changes, so it
/// goes in the system prompt once. His memory grows, his picture of the game is rewritten,
/// and the game itself starts and stops, so all of that has to arrive with the question.
///
/// A one shot process could put everything in the system prompt and used to. Keeping the two
/// apart costs nothing there and is the only thing that makes a long lived process possible.
pub struct Prepared<'a> {
    pub session: SessionId,
    /// False on the very first message of a thread, because there is nothing to resume yet.
    pub resume: bool,
    /// The question as asked.
    pub prompt: &'a str,
    /// Who Carl is. Fixed for the life of a process.
    pub identity: String,
    /// Everything that can change between turns.
    pub context: Option<String>,
    pub workdir: PathBuf,
}

impl Prepared<'_> {
    /// The whole system addition, for a process that only lives for one turn.
    pub fn all_in_system(&self) -> String {
        match &self.context {
            Some(c) => format!("{}\n\n{c}", self.identity),
            None => self.identity.clone(),
        }
    }

    /// The question with everything changeable in front of it, for a held open process.
    pub fn question_with_context(&self) -> String {
        match &self.context {
            Some(c) => format!("{c}\n\n---\n\n{}", self.prompt),
            None => self.prompt.to_string(),
        }
    }
}

impl Exchange<'_> {
    /// Records the question, prepares the turn, runs `ask`, records the outcome.
    pub fn run(self, ask: impl FnOnce(&Prepared<'_>) -> crate::Result<Answer>) -> Result<Answer> {
        // Kept before the author is handed to the log, which consumes it. Notes written this
        // turn are attributed to whoever is speaking, since memory is one pile and everybody
        // who can reach Carl writes into it.
        let speaker = self.author.clone().unwrap_or_default();

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

        // Who Carl is goes on every turn, not just spoken ones. The underlying binary
        // describes itself as a tool for software engineering, and without this Carl inherits
        // that and turns down questions about anything else.
        let mut identity = crate::brief::IDENTITY.to_string();
        if let Some(extra) = self.extra {
            identity.push_str("\n\n");
            identity.push_str(extra);
        }

        // Everything from here changes between turns, so it cannot live in a system prompt
        // that is fixed when a process starts.
        let mut extra_system = String::new();
        if let Some(notes) = memory.assemble()? {
            extra_system.push_str(&notes);
        }

        // Whenever a game is up or was up recently, rather than when the question looks like
        // it is about one. Guessing from keywords was the first attempt and it failed on the
        // first real question: "I need more iron plates" contains none of them, so Carl
        // answered about stone furnaces to somebody playing Sea Block, where there are no
        // ores to smelt. Recency is a fact and keyword lists are a guess.
        let game = crate::game::playing();

        // A game in a browser tab cannot be detected at all, so it is named by being told and
        // kept under this name instead. Carl writing "they are playing Slither" is the only
        // signal available, and it is a good one.
        let picture_name = game
            .as_ref()
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "current".to_string());

        if let Some(found) = &game
            && let Some(brief) = crate::game::brief(found)
        {
            extra_system.push_str("\n\n");
            extra_system.push_str(&brief);

            // What he made of it last time. Without this every look starts over and a
            // question like "is that better than before" has no answer available.
            match crate::game::seen::read(self.home, &picture_name) {
                Some(picture) => {
                    extra_system.push_str(&format!(
                        "\n\nWhere they were up to when you last heard, which may be out of \
                         date:\n{picture}"
                    ));
                }
                None => extra_system.push_str(
                    "\n\nYou do not know where they are up to in this game yet. The moment \
                     you find out, from a screenshot or from them saying so, record it.",
                ),
            }
        } else if let Some(picture) = crate::game::seen::read(self.home, &picture_name) {
            // Nothing detectable is running, but something was recorded. Almost always a
            // browser game, since a page in a tab leaves no trace anywhere a program can look.
            extra_system.push_str(&format!(
                "\n\n# The game\n\nNo game was detected running, which usually means it is in \
                 a browser tab. What you were told or last saw, which may be out of date:\n\
                 {picture}"
            ));
        }

        // Claude Code runs with this as its working directory, so anything it writes lands
        // somewhere predictable rather than wherever carl happened to be started from.
        let workdir = self.home.join("workspace");

        let prepared = Prepared {
            session: session.clone(),
            // A brand new thread has nothing to resume. Getting this wrong is the difference
            // between continuing a conversation and starting a second one silently.
            resume: !is_new,
            prompt: self.sent.unwrap_or(self.said),
            identity,
            context: (!extra_system.trim().is_empty()).then(|| extra_system.trim().to_string()),
            workdir,
        };

        let outcome = ask(&prepared);

        match outcome {
            Ok(mut answer) => {
                // Taken out here, once, rather than at each of the three places that show an
                // answer. Carl writes a note inside the reply he was already giving, which
                // costs nothing, works on every surface, and lets him decide, since he is the
                // only participant who knows whether something mattered.
                let kept = crate::remember::split(&answer.text);
                answer.text = kept.text;

                // Dropped before kept, so correcting a fact in one breath cannot delete the
                // replacement it just wrote when both slugify to the same name.
                for note in &kept.drop {
                    match forget_note(&memory, note) {
                        Ok(true) => eprintln!("  forgot: {note}"),
                        Ok(false) => eprintln!("  nothing to forget matching: {note}"),
                        Err(e) => eprintln!("  could not forget a note: {e}"),
                    }
                }

                // Written before the notes, because it is the cheap one and cannot fail in a
                // way worth abandoning the rest for.
                if let Some(picture) = &kept.seen
                    && let Err(e) = crate::game::seen::write(self.home, &picture_name, picture)
                {
                    eprintln!("  could not keep what was seen: {e}");
                }

                let mut lost = Vec::new();
                for note in &kept.keep {
                    let name = crate::remember::note_name(note);
                    if name.is_empty() {
                        eprintln!("  a note with no words in it was skipped: {note:?}");
                        continue;
                    }
                    match memory.write_from(&name, note, &speaker) {
                        Ok(path) => eprintln!("  remembered: {}", path.display()),
                        // Never fatal. Failing to keep a note must not lose the answer that
                        // came with it, which somebody is waiting for.
                        Err(e) => {
                            eprintln!("  could not keep a note: {e}");
                            lost.push(note.as_str());
                        }
                    }
                }

                // Said out loud rather than only to stderr. Carl has already told them he
                // wrote it down, because writing the `[remember]` line is what saving is, and
                // on the Slack path stderr goes to a service journal nobody is reading. Two
                // people then believe a fact is kept and it is not. See bug 20.
                if !lost.is_empty() {
                    answer.text.push_str(&format!(
                        "\n\n(I could not actually save that, so it is not remembered: {})",
                        lost.join("; ")
                    ));
                }

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

#[cfg(test)]
mod tests {
    use super::*;

    /// Naming and forgetting have to agree, and they now share one function so they cannot
    /// drift. This is the check that the shared one is actually the one being used here.
    #[test]
    fn a_note_can_be_forgotten_by_its_words_or_by_its_filename() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::open(dir.path()).unwrap();

        let fact = "JJ prefers two sentence answers";
        memory
            .write(&crate::remember::note_name(fact), fact)
            .unwrap();

        assert!(forget_note(&memory, fact).unwrap(), "by its words");
        assert!(!forget_note(&memory, fact).unwrap(), "and only once");

        memory
            .write(&crate::remember::note_name(fact), fact)
            .unwrap();
        assert!(
            forget_note(&memory, "jj-prefers-two-sentence-answers.md").unwrap(),
            "by the filename he was shown"
        );
    }

    /// Forgetting something that was never kept must say so rather than claim success, or
    /// Carl believes a wrong fact is gone and keeps being told it.
    #[test]
    fn forgetting_something_that_is_not_there_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let memory = Memory::open(dir.path()).unwrap();
        assert!(!forget_note(&memory, "something never written").unwrap());
    }

    /// A filename becomes a path.
    #[test]
    fn forgetting_cannot_reach_outside_the_memory_directory() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("secret.md");
        std::fs::write(&outside, "not a note").unwrap();

        let memory = Memory::open(dir.path().join("memory")).unwrap();
        let _ = forget_note(&memory, "../secret");
        assert!(
            outside.exists(),
            "must not have reached out of the directory"
        );
    }
}
