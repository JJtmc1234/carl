//! Stopping two agents talking to each other until somebody notices the bill.
//!
//! Carl replying to Alex is the whole point. Carl replying to Alex replying to Carl, four
//! hundred times, in a channel other people are reading, is the same feature with nothing
//! stopping it. Neither of them gets bored, neither runs out of things to say, and every
//! single turn is a paid model call.
//!
//! This is the third time the same shape has come up. The microphone hearing the speakers,
//! Carl seeing his own Slack messages, and now two agents feeding each other. The first two
//! were fixed by refusing to listen to yourself, which does not work here, because Alex
//! genuinely is somebody else and the whole point is to hear him.
//!
//! So the rule is not about who is talking. It is about how long it has been since a person
//! was involved. A human message in a thread resets the count. Without one, the agents get a
//! fixed number of turns and then Carl goes quiet, in that thread only, and says so.

use std::collections::HashMap;

use crate::ThreadId;

/// How many times Carl will answer another agent before a person has to say something.
///
/// Six is enough for a real exchange, a question, an answer, a follow up and a conclusion,
/// and small enough that the worst case is six model calls rather than a runaway.
pub const DEFAULT_LIMIT: u32 = 6;

/// Tracks how far each thread has drifted from the last thing a person said.
#[derive(Debug)]
pub struct Patience {
    limit: u32,
    /// Consecutive agent replies per thread. Deliberately only in memory, so restarting Carl
    /// clears it. Erring towards forgetting is right: the failure of forgetting is a few
    /// extra messages, and the failure of remembering forever is a thread that can never be
    /// used again.
    streak: HashMap<ThreadId, u32>,
}

impl Default for Patience {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT)
    }
}

impl Patience {
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            streak: HashMap::new(),
        }
    }

    /// Whether Carl should answer this one.
    ///
    /// Call once per incoming question. A person speaking resets the thread, because a person
    /// in the conversation is the thing that makes it a conversation rather than a loop.
    pub fn allows(&mut self, thread: &ThreadId, from_agent: bool) -> bool {
        if !from_agent {
            self.streak.remove(thread);
            return true;
        }

        let n = self.streak.entry(thread.clone()).or_insert(0);
        *n += 1;
        *n <= self.limit
    }

    /// How many agent turns are left in this thread, for saying so out loud.
    pub fn left(&self, thread: &ThreadId) -> u32 {
        self.limit
            .saturating_sub(self.streak.get(thread).copied().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(name: &str) -> ThreadId {
        ThreadId::new(name).unwrap()
    }

    /// A person is never rate limited. Only agents are.
    #[test]
    fn a_person_can_talk_as_long_as_they_like() {
        let mut p = Patience::new(2);
        let t = thread("slack-C1-1.1");
        for _ in 0..50 {
            assert!(p.allows(&t, false));
        }
    }

    /// The whole reason this file exists. Two agents left alone must stop.
    #[test]
    fn two_agents_alone_run_out_of_turns() {
        let mut p = Patience::new(3);
        let t = thread("slack-C1-1.1");

        assert!(p.allows(&t, true));
        assert!(p.allows(&t, true));
        assert!(p.allows(&t, true));
        assert!(!p.allows(&t, true), "the fourth must be refused");
        assert!(!p.allows(&t, true), "and it must stay refused");
    }

    /// A person joining in is what makes it a conversation rather than a loop, so it starts
    /// the allowance again.
    #[test]
    fn a_person_speaking_gives_the_agents_another_go() {
        let mut p = Patience::new(2);
        let t = thread("slack-C1-1.1");

        assert!(p.allows(&t, true));
        assert!(p.allows(&t, true));
        assert!(!p.allows(&t, true));

        assert!(p.allows(&t, false), "a person is always heard");
        assert!(p.allows(&t, true), "and the agents may carry on");
    }

    /// One runaway thread must not silence Carl everywhere else. Hunter and Alex arguing in
    /// one channel cannot be allowed to stop Carl answering JJ in another.
    #[test]
    fn threads_are_counted_separately() {
        let mut p = Patience::new(1);
        let noisy = thread("slack-C1-1.1");
        let quiet = thread("slack-C2-2.2");

        assert!(p.allows(&noisy, true));
        assert!(!p.allows(&noisy, true));
        assert!(p.allows(&quiet, true), "a different thread is unaffected");
    }

    #[test]
    fn the_remaining_turns_can_be_reported() {
        let mut p = Patience::new(3);
        let t = thread("slack-C1-1.1");
        assert_eq!(p.left(&t), 3);
        p.allows(&t, true);
        assert_eq!(p.left(&t), 2);
        p.allows(&t, false);
        assert_eq!(p.left(&t), 3, "a person resets it");
    }
}
