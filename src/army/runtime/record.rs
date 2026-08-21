//! What the supervisor knows about one agent's process, and where it is kept.
//!
//! ```text
//!   <home>/run/events.jsonl        the journal
//!   <home>/run/agents/a-1f2e....json   one of these per agent that has ever been started
//! ```
//!
//! **Not in the agent's folder, and that is the whole reason it is here.** An agent writes its
//! own `state.json` every turn. If its process record sat next to that file, an agent could
//! write down that it is running, or that it is stopped, or that its session is some other
//! session. None of those would be true, but all of them would be believed by whatever read the
//! file next. Under `run/` it is the supervisor's, next to the journal, which is the other thing
//! in this system that describes agents and is not written by them.
//!
//! **Named by id rather than by name.** An agent's name is expected to become changeable. A
//! directory of records keyed by name would orphan one the first time that happened, and the
//! orphan would look exactly like an agent that had never been started.
//!
//! **Three lifetimes, kept apart on purpose.** The agent id outlives everything. The session
//! outlives any number of processes and is what `--resume` continues. The process is the most
//! disposable thing in the system and is expected to be replaced. Squashing any two of those
//! together is how a restart quietly becomes a different agent, or how a replaced process
//! quietly loses a conversation.

use serde::{Deserialize, Serialize};

use crate::SessionId;
use crate::army::personnel::AgentId;

/// Where one agent's process is up to.
///
/// Deliberately without a "waiting to restart" state. When the next attempt is due is worked out
/// from the exit and the attempt count rather than stored, because a deadline written into a file
/// is a second clock, and two clocks disagree the first time one write is missed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "lifecycle", rename_all = "snake_case")]
pub enum Lifecycle {
    /// Has a record, has never had a process. The state a record is born in.
    Never,
    /// A process the supervisor believes is alive.
    ///
    /// Believes rather than knows: this is a file, and the process it names can exit a
    /// microsecond after it is written. `started` is what makes checking possible, because a
    /// pid on its own cannot be told apart from the pid that replaced it.
    Running {
        pid: u32,
        /// Clock ticks since boot, from `/proc/<pid>/stat`. An identifier, not a duration.
        started: u64,
        /// Unix seconds when the supervisor started it.
        since: u64,
    },
    /// The process ended. Whether that was a crash or a clean exit, it is not running.
    Exited {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<i32>,
        at: u64,
    },
    /// Started too many times, failed too many times, and the supervisor has stopped trying.
    ///
    /// Not a crash and not a decision about the work. It is the supervisor admitting that
    /// starting this agent is not something it can fix by doing it again.
    Degraded { why: String },
    /// Deliberately not running. The supervisor will not start it until somebody says so.
    Stopped { why: String },
}

impl Lifecycle {
    /// A short name, for counting without matching every variant.
    pub fn kind(&self) -> &'static str {
        match self {
            Lifecycle::Never => "never",
            Lifecycle::Running { .. } => "running",
            Lifecycle::Exited { .. } => "exited",
            Lifecycle::Degraded { .. } => "degraded",
            Lifecycle::Stopped { .. } => "stopped",
        }
    }

    /// The pid this claims, when it claims one. Claims, because it may already be wrong.
    pub fn pid(&self) -> Option<u32> {
        match self {
            Lifecycle::Running { pid, .. } => Some(*pid),
            _ => None,
        }
    }
}

/// Everything the supervisor holds about one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    /// The durable agent this is about. Also the filename.
    pub agent: AgentId,
    /// What that agent was called when this was written.
    ///
    /// For whoever opens `run/agents/` and needs to know which file is Nora's. Nothing reads it
    /// back, and nothing decides anything from it, because the id is the identifier.
    pub name: String,
    pub lifecycle: Lifecycle,
    /// The conversation, which outlives every process that ever serves it.
    ///
    /// Absent before the first start. Present afterwards even while nothing is running, because
    /// that is the whole point: it is what the next process resumes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionId>,
    /// Sessions given up on, kept rather than deleted.
    ///
    /// A session that could not be resumed is the best evidence there is about why, and Claude
    /// Code still has the transcript. Deleting the id would mean nobody could ever go and look.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abandoned: Vec<SessionId>,
    /// Consecutive starts that did not stick. Reset by a process that stayed up.
    #[serde(default)]
    pub attempts: u32,
    /// The pid of the supervisor that last wrote this.
    ///
    /// So a record can say who owns it. A supervisor reading a record written by a pid that is
    /// not its own is reading about a process it has no pipes to, however alive that process is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervisor: Option<u32>,
    pub updated_at: u64,
}

impl Runtime {
    /// A record for an agent that has never been started.
    pub fn never(agent: AgentId, name: impl Into<String>, now: u64) -> Self {
        Self {
            agent,
            name: name.into(),
            lifecycle: Lifecycle::Never,
            session: None,
            abandoned: Vec::new(),
            attempts: 0,
            supervisor: None,
            updated_at: now,
        }
    }

    /// Whether the supervisor asking is the one that started what this describes.
    ///
    /// A record left by a supervisor that has since exited names a process which may well still
    /// be alive, and which nothing can talk to any more, because its pipes went with its parent.
    pub fn owned_by(&self, supervisor: u32) -> bool {
        self.supervisor == Some(supervisor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id() -> AgentId {
        AgentId::fresh().unwrap()
    }

    #[test]
    fn a_record_round_trips_through_every_lifecycle() {
        for lifecycle in [
            Lifecycle::Never,
            Lifecycle::Running {
                pid: 42,
                started: 987,
                since: 100,
            },
            Lifecycle::Exited {
                code: Some(1),
                at: 200,
            },
            Lifecycle::Exited {
                code: None,
                at: 200,
            },
            Lifecycle::Degraded {
                why: "will not start".into(),
            },
            Lifecycle::Stopped {
                why: "asked to".into(),
            },
        ] {
            let mut before = Runtime::never(id(), "nora", 100);
            before.lifecycle = lifecycle;
            before.session = Some(SessionId::fresh().unwrap());
            let text = serde_json::to_string_pretty(&before).unwrap();
            assert_eq!(serde_json::from_str::<Runtime>(&text).unwrap(), before);
        }
    }

    /// The record an agent would most like to be able to write, so it holds nothing worth
    /// forging and refuses anything it does not recognise rather than ignoring it.
    #[test]
    fn a_runtime_record_cannot_carry_authority() {
        for field in ["rank", "reports_to", "granted", "may_implement", "sudo"] {
            let mut raw = serde_json::to_value(Runtime::never(id(), "nora", 1)).unwrap();
            raw.as_object_mut()
                .unwrap()
                .insert(field.into(), serde_json::json!("chief"));
            assert!(
                serde_json::from_value::<Runtime>(raw).is_err(),
                "{field} should not load"
            );
        }
    }

    /// A running process and an owned running process are different things, and a supervisor
    /// that cannot tell them apart will either adopt a stranger or abandon its own child.
    #[test]
    fn a_record_from_another_supervisor_is_not_owned() {
        let mut record = Runtime::never(id(), "nora", 1);
        record.supervisor = Some(1000);
        assert!(record.owned_by(1000));
        assert!(!record.owned_by(1001));

        record.supervisor = None;
        assert!(!record.owned_by(1000), "nobody claimed it");
    }
}
