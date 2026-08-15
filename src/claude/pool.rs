//! One `claude` process per task, alive for as long as Carl is.
//!
//! A task here is a thread: the voice is one, and every Slack conversation is one. Each keeps
//! its own process for the whole run, so the second question in a conversation costs nothing
//! to set up and the model has everything already in front of it.
//!
//! Measured, that is most of the delay in a spoken exchange. A fresh process reaches its first
//! token in 2.8 seconds. A process that is already running reaches it in 0.97.
//!
//! Two things this must get right, and they pull against each other.
//!
//! A process is not free. Each one peaked at about 195MB, so holding one open per Slack thread
//! forever would eventually be every thread anybody ever opened. The pool keeps a fixed number
//! and closes the least recently used, which for the voice means it is never closed, because
//! it is used constantly.
//!
//! And a long lived process has time to die. It can be killed, run out of memory, or be
//! restarted underneath. So a dead one is reopened rather than reported, and the reopen
//! resumes the conversation rather than starting a new one.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::{Answer, Flow, Runner, Session};
use crate::{Result, SessionId, ThreadId};

/// How many conversations to keep open at once.
///
/// Small on purpose. The voice is one, whoever is being talked to in Slack is another, and
/// beyond that a conversation nobody has touched in a while can afford to pay for its own
/// restart. Every open one costs memory whether it is being used or not.
pub const KEEP_OPEN: usize = 4;

pub struct Pool {
    runner: Runner,
    workdir: PathBuf,
    /// Fixed for the life of every session in here, because a system prompt is set when the
    /// process starts and cannot be changed afterwards. Anything that varies goes in the
    /// message instead.
    system: String,
    open: Vec<(ThreadId, Session)>,
    /// Most recently used last.
    order: VecDeque<ThreadId>,
    limit: usize,
}

impl Pool {
    pub fn new(runner: Runner, workdir: impl Into<PathBuf>, system: impl Into<String>) -> Self {
        Self {
            runner,
            workdir: workdir.into(),
            system: system.into(),
            open: Vec::new(),
            order: VecDeque::new(),
            limit: KEEP_OPEN,
        }
    }

    pub fn keeping(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }

    /// Asks a question in a thread, starting the conversation if it is not already running.
    ///
    /// `resume` says whether Claude has seen this session before, which decides whether the
    /// process pins a new one or picks up an existing transcript. Getting it wrong is the
    /// difference between continuing a conversation and silently starting a second.
    pub fn ask(
        &mut self,
        thread: &ThreadId,
        session: &SessionId,
        resume: bool,
        prompt: &str,
        on_text: &mut dyn FnMut(&str) -> Flow,
        while_waiting: &mut dyn FnMut() -> Flow,
    ) -> Result<Answer> {
        self.ensure(thread, session, resume)?;
        self.touch(thread);

        let attempt = {
            let live = self.session_for(thread).expect("just ensured");
            live.ask(prompt, on_text, while_waiting)
        };

        match attempt {
            Ok(answer) => Ok(answer),
            Err(e) => {
                // A long lived process has time to be killed by something else, and the
                // failure arrives as a broken pipe halfway through a question somebody has
                // just asked out loud. One reopen and one retry, resuming rather than
                // starting over, so the conversation survives.
                eprintln!("  the session for {thread} failed, reopening: {e}");
                self.close(thread);
                self.ensure(thread, session, true)?;
                let live = self.session_for(thread).expect("just reopened");
                live.ask(prompt, on_text, while_waiting)
            }
        }
    }

    /// Reopens conversations that were open before a restart.
    ///
    /// Restarting loses every held open process, and the first message to each thread
    /// afterwards pays the full startup again. The person who notices is whoever was mid
    /// conversation when the service was restarted, which during a day of building is
    /// constantly.
    ///
    /// Spawning is quick. What takes 0.8 seconds is the child reading its own config, and that
    /// happens inside the child, so starting several here costs almost nothing and they warm
    /// themselves while nobody is waiting.
    ///
    /// Always resumed rather than pinned, because a thread being warmed has by definition been
    /// talked in before.
    pub fn warm(&mut self, threads: &[(ThreadId, SessionId)]) {
        for (thread, session) in threads.iter().take(self.limit) {
            match self.ensure(thread, session, true) {
                Ok(()) => eprintln!("  reopened {thread}"),
                // Not fatal, and not retried. A thread that will not reopen simply opens when
                // somebody asks something in it, which is what used to happen every time.
                Err(e) => eprintln!("  could not reopen {thread}: {e}"),
            }
        }
    }

    /// Closes a conversation, so the next question starts a new process.
    pub fn close(&mut self, thread: &ThreadId) {
        self.open.retain(|(t, _)| t != thread);
        self.order.retain(|t| t != thread);
    }

    fn session_for(&mut self, thread: &ThreadId) -> Option<&mut Session> {
        self.open
            .iter_mut()
            .find(|(t, _)| t == thread)
            .map(|(_, s)| s)
    }

    /// Makes sure there is a live process for this thread.
    fn ensure(&mut self, thread: &ThreadId, session: &SessionId, resume: bool) -> Result<()> {
        // A process that has died is worse than none, because everything downstream believes
        // it is talking to something.
        if let Some(live) = self.session_for(thread)
            && !live.alive()
        {
            self.close(thread);
        }
        if self.session_for(thread).is_some() {
            return Ok(());
        }

        self.evict_until_room();

        let started = self
            .runner
            .open_session(session, &self.workdir, &self.system, resume)?;
        self.open.push((thread.clone(), started));
        self.order.push_back(thread.clone());
        Ok(())
    }

    fn evict_until_room(&mut self) {
        while self.open.len() >= self.limit {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.open.retain(|(t, _)| *t != oldest);
        }
    }

    fn touch(&mut self, thread: &ThreadId) {
        self.order.retain(|t| t != thread);
        self.order.push_back(thread.clone());
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(limit: usize) -> Pool {
        Pool::new(
            Runner::at("/nonexistent/definitely-not-claude"),
            "/tmp",
            "you are carl",
        )
        .keeping(limit)
    }

    fn thread(name: &str) -> ThreadId {
        ThreadId::new(name).unwrap()
    }

    /// A limit of zero would mean evicting the session being opened, so it is raised to one.
    #[test]
    fn the_limit_is_never_zero() {
        let p = pool(0);
        assert_eq!(p.limit, 1);
    }

    /// The voice is used constantly, so least recently used means it is never the one closed,
    /// which is the behaviour that matters most.
    #[test]
    fn the_least_recently_used_is_the_one_closed() {
        let mut p = pool(2);
        // Faked rather than opened, because opening needs a real claude and this is about the
        // bookkeeping, which is where an eviction rule goes wrong.
        for name in ["voice", "slack-a", "slack-b"] {
            p.order.push_back(thread(name));
        }
        p.touch(&thread("voice"));

        assert_eq!(
            p.order.iter().map(|t| t.to_string()).collect::<Vec<_>>(),
            vec!["slack-a", "slack-b", "voice"],
            "using one must move it to the end"
        );
    }

    #[test]
    fn using_a_thread_twice_does_not_list_it_twice() {
        let mut p = pool(4);
        p.touch(&thread("voice"));
        p.touch(&thread("voice"));
        assert_eq!(p.order.len(), 1);
    }

    /// Closing forgets it in both places. Leaving it in one is how a thread ends up unable to
    /// reopen, or evicting something that is not there.
    #[test]
    fn closing_forgets_it_everywhere() {
        let mut p = pool(4);
        p.touch(&thread("voice"));
        p.close(&thread("voice"));
        assert!(p.order.is_empty());
        assert_eq!(p.open_count(), 0);
    }

    /// A process that cannot be started is an error rather than a silent nothing, since
    /// everything downstream would otherwise believe it is talking to something.
    #[test]
    fn a_session_that_cannot_start_says_so() {
        let mut p = pool(2);
        let s = SessionId::fresh().unwrap();
        let err = p.ask(
            &thread("voice"),
            &s,
            false,
            "hello",
            &mut |_| Flow::Continue,
            &mut || Flow::Continue,
        );
        assert!(err.is_err());
        assert_eq!(p.open_count(), 0, "nothing should be left half open");
    }
}
