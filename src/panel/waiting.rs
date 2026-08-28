//! Permission requests the panel has been asked and JJ has not answered.
//!
//! One place, shared by every connection, because the hook asks on one socket and the answer
//! arrives on another. The hook parks on a channel rather than polling, so an answer reaches it
//! the moment it is given rather than up to a tick later, and a tool call is held still for as
//! little time as the person takes.
//!
//! Bounded on purpose. A backend that accumulated questions nobody answered would hold a
//! process open for each, so the oldest is dropped and refused rather than queued forever.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::permission::{Request, Verdict};

/// How many can be outstanding before the oldest is refused to make room.
///
/// More than one because a turn can ask twice before anybody looks up. Small because each one
/// is a process sitting still, and a screen of unanswered questions is not a thing anybody
/// reads, it is a thing they dismiss.
pub const MOST: usize = 8;

/// How many settled outcomes are kept for panels that were not the one to answer.
///
/// A panel showing a question needs to be told when it stops being a question, including when
/// somebody answered it on another screen or it ran out of time. Bounded because this is a
/// courtesy for connected panels and not a record: the journal is the record.
pub const REMEMBERED: usize = 64;

#[derive(Default)]
pub struct Waiting {
    inner: Mutex<HashMap<String, Entry>>,
    /// What has been settled, in order, so a subscriber can forward only what is new to it.
    ///
    /// Numbered from one by its own counter rather than by journal sequence, because a question
    /// that timed out is not a thing that happened to the army and does not belong in the journal.
    settled: Mutex<(u64, Vec<Outcome>)>,
}

/// A question that has stopped being one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub at: u64,
    pub id: String,
    pub verdict: Verdict,
}

struct Entry {
    request: Request,
    answer: Sender<Verdict>,
}

impl Waiting {
    pub fn new() -> Self {
        Self::default()
    }

    /// Writes down that a question is over, for panels that were not the one to end it.
    fn settle(&self, id: &str, verdict: Verdict) {
        if let Ok(mut kept) = self.settled.lock() {
            kept.0 += 1;
            let at = kept.0;
            kept.1.push(Outcome {
                at,
                id: id.to_string(),
                verdict,
            });
            let over = kept.1.len().saturating_sub(REMEMBERED);
            kept.1.drain(..over);
        }
    }

    /// Outcomes newer than a subscriber has been told about, and the new high water mark.
    ///
    /// A subscriber starting at zero is deliberately not sent the backlog. It is connecting now,
    /// and being told about a question it never saw, already answered, is noise.
    pub fn settled_after(&self, after: u64) -> (Vec<Outcome>, u64) {
        let Ok(kept) = self.settled.lock() else {
            return (Vec::new(), after);
        };
        let fresh: Vec<Outcome> = kept.1.iter().filter(|o| o.at > after).cloned().collect();
        (fresh, kept.0)
    }

    /// Where the outcome list has got to, for a subscriber that wants only what happens next.
    pub fn settled_now(&self) -> u64 {
        self.settled.lock().map(|k| k.0).unwrap_or(0)
    }

    /// Records a question and hands back the end of the channel its answer will arrive on.
    ///
    /// Returns `None` when the same id is already outstanding, which means something is
    /// retrying rather than asking, and answering it twice would be worse than refusing it.
    pub fn ask(&self, request: Request) -> Option<Receiver<Verdict>> {
        let (tx, rx) = channel();
        let mut held = self.inner.lock().ok()?;
        if held.contains_key(&request.id) {
            return None;
        }

        // Room made by refusing the oldest, so the number of parked processes has a ceiling.
        while held.len() >= MOST {
            let oldest = held
                .values()
                .min_by_key(|e| e.request.at)
                .map(|e| e.request.id.clone())?;
            if let Some(e) = held.remove(&oldest) {
                let _ = e.answer.send(Verdict::Deny);
                drop(held);
                self.settle(&oldest, Verdict::Deny);
                held = self.inner.lock().ok()?;
            }
        }

        held.insert(
            request.id.clone(),
            Entry {
                request,
                answer: tx,
            },
        );
        Some(rx)
    }

    /// Answers one, waking whatever is parked on it.
    ///
    /// False means nothing was waiting under that id: it was answered already, or it timed out
    /// and refused itself. Either way the answer has nowhere to go and saying so is better than
    /// pretending it landed.
    pub fn answer(&self, id: &str, verdict: Verdict) -> bool {
        let Ok(mut held) = self.inner.lock() else {
            return false;
        };
        let entry = held.remove(id);
        drop(held);
        match entry {
            Some(entry) => {
                self.settle(id, verdict);
                // False when the asker gave up first. The question is still over, and the
                // outcome is still written down, so every panel stops showing it either way.
                entry.answer.send(verdict).is_ok()
            }
            None => false,
        }
    }

    /// Gives up on one, refusing it. Called by the hook when it has waited long enough.
    pub fn give_up(&self, id: &str) {
        let gone = self
            .inner
            .lock()
            .map(|mut held| held.remove(id).is_some())
            .unwrap_or(false);
        // Only if it was still outstanding. Giving up on a question somebody answered a moment
        // ago must not overwrite their answer with a refusal.
        if gone {
            self.settle(id, Verdict::Deny);
        }
    }

    /// Everything outstanding, oldest first, so a panel that has just connected can show what
    /// is already waiting rather than only what arrives next.
    pub fn outstanding(&self) -> Vec<Request> {
        let Ok(held) = self.inner.lock() else {
            return Vec::new();
        };
        let mut all: Vec<Request> = held.values().map(|e| e.request.clone()).collect();
        all.sort_by_key(|r| r.at);
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, at: u64) -> Request {
        Request {
            id: id.into(),
            tool: "Bash".into(),
            detail: "python3 -c 'print(1)'".into(),
            surface: "panel".into(),
            at,
        }
    }

    #[test]
    fn an_answer_reaches_whatever_is_waiting_on_it() {
        let w = Waiting::new();
        let rx = w.ask(request("a", 1)).expect("asked");
        assert!(w.answer("a", Verdict::Allow));
        assert_eq!(rx.recv().unwrap(), Verdict::Allow);
    }

    #[test]
    fn answering_something_nobody_asked_says_so() {
        let w = Waiting::new();
        assert!(!w.answer("nothing", Verdict::Allow));
    }

    #[test]
    fn the_same_question_twice_is_refused_rather_than_doubled() {
        let w = Waiting::new();
        let _first = w.ask(request("a", 1)).expect("asked");
        assert!(
            w.ask(request("a", 2)).is_none(),
            "a retry is not a new question"
        );
    }

    /// Each one parked is a process sitting still, so there has to be a ceiling.
    #[test]
    fn the_oldest_is_refused_to_make_room_rather_than_queueing_forever() {
        let w = Waiting::new();
        let first = w.ask(request("oldest", 1)).expect("asked");
        let mut rest = Vec::new();
        for n in 2..=(MOST as u64) {
            rest.push(w.ask(request(&format!("r{n}"), n)).expect("asked"));
        }
        assert_eq!(w.outstanding().len(), MOST);

        let _pushed = w.ask(request("newest", 99)).expect("asked");
        assert_eq!(
            first.recv().unwrap(),
            Verdict::Deny,
            "the one pushed out is refused, not forgotten"
        );
        assert_eq!(w.outstanding().len(), MOST);
    }

    #[test]
    fn what_is_outstanding_comes_back_oldest_first() {
        let w = Waiting::new();
        let _a = w.ask(request("b", 20));
        let _b = w.ask(request("a", 10));
        let ids: Vec<String> = w.outstanding().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn a_panel_that_did_not_answer_is_still_told_it_is_over() {
        let w = Waiting::new();
        let watching_from = w.settled_now();
        let _rx = w.ask(request("a", 1));
        w.answer("a", Verdict::Allow);

        let (fresh, upto) = w.settled_after(watching_from);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, "a");
        assert_eq!(fresh[0].verdict, Verdict::Allow);
        assert!(w.settled_after(upto).0.is_empty(), "and not twice");
    }

    /// The asker walking away must not rewrite an answer that was already given.
    #[test]
    fn giving_up_after_an_answer_does_not_turn_it_into_a_refusal() {
        let w = Waiting::new();
        let from = w.settled_now();
        let _rx = w.ask(request("a", 1));
        w.answer("a", Verdict::Allow);
        w.give_up("a");

        let (fresh, _) = w.settled_after(from);
        assert_eq!(fresh.len(), 1, "one outcome, not two: {fresh:?}");
        assert_eq!(fresh[0].verdict, Verdict::Allow);
    }

    #[test]
    fn a_question_that_ran_out_of_time_is_recorded_as_refused() {
        let w = Waiting::new();
        let from = w.settled_now();
        let _rx = w.ask(request("a", 1));
        w.give_up("a");
        assert_eq!(w.settled_after(from).0[0].verdict, Verdict::Deny);
    }

    #[test]
    fn giving_up_leaves_nothing_behind() {
        let w = Waiting::new();
        let _rx = w.ask(request("a", 1));
        w.give_up("a");
        assert!(w.outstanding().is_empty());
        assert!(!w.answer("a", Verdict::Allow), "there is nothing to answer");
    }
}
