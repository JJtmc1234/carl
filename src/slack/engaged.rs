//! Threads Carl is already in, where he no longer needs to be addressed by name.
//!
//! Saying his name is how a conversation starts. Making you say it on every single message
//! afterwards is not a conversation, it is a series of unrelated questions that happen to be
//! next to each other. The voice already knows this: "hey carl" wakes him and then you just
//! talk until you say "end conversation".
//!
//! A Slack thread is the same shape and needs the same rule. Once Carl has answered in a
//! thread, that thread is his.
//!
//! Deliberately only in memory. Restarting Carl means he needs addressing again, and erring
//! that way is right, because the cost of forgetting is one extra use of his name and the
//! cost of remembering wrongly is butting into a conversation from last week.

use std::collections::{HashSet, VecDeque};

/// How many threads to stay engaged in at once.
///
/// Bounded because a long lived process in a busy workspace would otherwise remember every
/// thread it ever touched. The oldest is dropped, which at worst means Carl needs addressing
/// by name again in a thread nobody has touched in a hundred conversations.
const REMEMBERED: usize = 128;

#[derive(Debug, Default)]
pub struct Engaged {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl Engaged {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether Carl is already part of this thread.
    pub fn contains(&self, thread_ts: &str) -> bool {
        self.set.contains(thread_ts)
    }

    /// Remembers that Carl has spoken here.
    pub fn join(&mut self, thread_ts: &str) {
        if !self.set.insert(thread_ts.to_string()) {
            return;
        }
        self.order.push_back(thread_ts.to_string());
        if self.order.len() > REMEMBERED
            && let Some(old) = self.order.pop_front()
        {
            self.set.remove(&old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The thing JJ hit. He said "carl how are you", got an answer, then asked a follow up in
    /// the same thread without the name and was ignored.
    #[test]
    fn a_thread_carl_answered_in_stays_his() {
        let mut e = Engaged::new();
        assert!(!e.contains("1700000000.1"), "not his until he speaks");
        e.join("1700000000.1");
        assert!(e.contains("1700000000.1"));
    }

    #[test]
    fn other_threads_are_unaffected() {
        let mut e = Engaged::new();
        e.join("1700000000.1");
        assert!(!e.contains("1700000000.2"));
    }

    #[test]
    fn joining_twice_is_harmless() {
        let mut e = Engaged::new();
        e.join("a");
        e.join("a");
        assert_eq!(e.order.len(), 1, "no duplicate should be queued");
        assert!(e.contains("a"));
    }

    /// A long lived process in a busy workspace must not grow forever.
    #[test]
    fn the_oldest_thread_is_forgotten_first() {
        let mut e = Engaged::new();
        for i in 0..REMEMBERED + 10 {
            e.join(&format!("t{i}"));
        }
        assert_eq!(e.set.len(), REMEMBERED);
        assert!(!e.contains("t0"), "the oldest should be gone");
        assert!(e.contains(&format!("t{}", REMEMBERED + 9)), "newest kept");
    }
}
