//! What an agent knows between one turn and the next.
//!
//! This is the file that changes constantly and is written by the agent's own run, which makes
//! it the one an agent could most plausibly get at. It is therefore built so that getting at
//! it buys nothing.
//!
//! **There is no rank here, no reporting line, and no permission.** Not hidden, not validated,
//! absent. All three live in `army::org`, which is a compiled in table, so there is no file
//! anywhere an agent could edit to promote itself. That is a stronger thing to be able to say
//! than "the check would catch it".
//!
//! **There is no task status here either.** A task's status belongs to the task, and copying
//! it into the agent's folder would make two places disagree the first time one write failed.
//! What is here is which task the agent is holding. The status is read from the task.

use serde::{Deserialize, Serialize};

use crate::army::TaskId;

/// How many recent notes are kept. Enough to see what just happened, few enough that this
/// stays a small file somebody can read rather than a second log.
pub const RECENT_KEPT: usize = 5;

/// The longest one note may be. Operational state, not a transcript.
pub const NOTE_LIMIT: usize = 200;

/// Everything about an agent that changes between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    /// The task this agent is holding, if any. The task itself, and its status, live in the
    /// task record. This is the reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub holding: Option<TaskId>,
    /// The last few things that happened, newest last. Bounded on write.
    #[serde(default)]
    pub recent: Vec<String>,
    /// Unix seconds when this agent was enlisted.
    pub enlisted_at: u64,
    /// Unix seconds when this file was last written.
    pub updated_at: u64,
}

impl State {
    /// A freshly enlisted agent, holding nothing.
    pub fn fresh(now: u64) -> Self {
        Self {
            holding: None,
            recent: Vec::new(),
            enlisted_at: now,
            updated_at: now,
        }
    }

    /// Records that this agent has taken a task on.
    pub fn take_up(&mut self, task: &TaskId, now: u64) {
        self.note(format!("took up {task}"), now);
        self.holding = Some(task.clone());
    }

    /// Records that this agent is no longer holding anything.
    pub fn put_down(&mut self, why: &str, now: u64) {
        let had = match &self.holding {
            Some(task) => format!("put down {task}: {why}"),
            None => format!("put down nothing: {why}"),
        };
        self.note(had, now);
        self.holding = None;
    }

    /// Records one line of operational state, dropping the oldest once the list is full.
    ///
    /// Bounded and truncated on the way in, because this file is read on every load. An
    /// unbounded pile here is a file that grows forever and stops being worth opening, and the
    /// caller is usually holding whatever a model just said.
    pub fn note(&mut self, what: impl Into<String>, now: u64) {
        let mut what = what.into();
        if what.len() > NOTE_LIMIT {
            what.truncate(NOTE_LIMIT);
            what.push_str("...");
        }
        self.recent.push(what);
        let over = self.recent.len().saturating_sub(RECENT_KEPT);
        self.recent.drain(..over);
        self.updated_at = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> TaskId {
        TaskId::quoted("task-1")
    }

    #[test]
    fn a_fresh_agent_holds_nothing() {
        let s = State::fresh(100);
        assert!(s.holding.is_none());
        assert_eq!(s.enlisted_at, 100);
        assert_eq!(s.updated_at, 100);
    }

    #[test]
    fn taking_up_and_putting_down_move_the_held_task() {
        let mut s = State::fresh(100);
        s.take_up(&task(), 200);
        assert_eq!(s.holding.as_ref().unwrap().as_str(), "task-1");
        assert_eq!(s.updated_at, 200);

        s.put_down("accepted", 300);
        assert!(s.holding.is_none());
        assert!(s.recent.iter().any(|r| r.contains("accepted")));
    }

    /// This file is read on every load, so it stays small on its own rather than by anybody
    /// remembering to trim it.
    #[test]
    fn recent_notes_are_bounded() {
        let mut s = State::fresh(0);
        for i in 0..20 {
            s.note(format!("thing {i}"), i);
        }
        assert_eq!(s.recent.len(), RECENT_KEPT);
        assert_eq!(s.recent.last().unwrap(), "thing 19", "the newest is kept");
        assert_eq!(s.recent[0], "thing 15", "the oldest is dropped");
    }

    #[test]
    fn a_wall_of_model_output_is_cut_down() {
        let mut s = State::fresh(0);
        s.note("x".repeat(40_000), 1);
        assert!(s.recent[0].len() <= NOTE_LIMIT + 3);
    }

    #[test]
    fn state_round_trips() {
        let mut before = State::fresh(100);
        before.take_up(&task(), 200);
        let text = serde_json::to_string_pretty(&before).unwrap();
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), before);
    }

    /// The escalation attempt this whole layer is shaped around, tried the way an agent could
    /// actually try it. None of these fields exist, so the file does not load at all.
    #[test]
    fn a_state_file_cannot_carry_authority() {
        for field in ["rank", "reports_to", "may_delegate_to", "granted", "owner"] {
            let mut raw = serde_json::to_value(State::fresh(1)).unwrap();
            raw.as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!("chief"));
            assert!(
                serde_json::from_value::<State>(raw).is_err(),
                "{field} should not load"
            );
        }
    }
}
