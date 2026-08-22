//! What survived into this process, said in three parts because they fail separately.
//!
//! An agent that has just been started is not one fact. A process was replaced, which is normal
//! and costs nothing. A conversation was continued, or was not, and those are very different
//! agents to talk to afterwards. A memory folder was there, or was not, and an agent whose folder
//! has gone has lost everything it wrote down whatever happened to its conversation.
//!
//! Collapsing those into one word is the temptation this module exists to refuse. "Restarted"
//! covers a process that came back with its whole conversation intact and a process that came
//! back knowing nothing, and the difference between those two is most of what somebody reading
//! the panel wants to know.
//!
//! **None of this is identity.** The agent is the same agent through all of it. `AgentId` is
//! minted once and never changes, and nothing here is allowed to suggest otherwise. This is about
//! how much of what that agent had is still with it.

use serde::{Deserialize, Serialize};

use super::policy::Start;
use super::record::{Lifecycle, Runtime};

/// Whether this is the agent's first process or a replacement for one that ended.
///
/// The least interesting of the three, and it is here because leaving it out would make the other
/// two ambiguous: a fresh conversation on a first process is an agent that has just been born,
/// and a fresh conversation on a replacement process is an agent that lost one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Process {
    First,
    Replaced,
}

/// What happened to the conversation the model is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Session {
    /// There was no conversation to continue. Nothing was lost.
    Fresh,
    /// The recorded conversation was continued into this process. The normal case, and the whole
    /// point of keeping a session id separately from a pid.
    Resumed,
    /// The recorded conversation could not be continued and was set aside. The agent is the same
    /// agent, with the same memory folder, and it does not remember being asked anything.
    Replaced,
}

/// Whether the agent's own folder was there when its process started.
///
/// A missing summary is a warning rather than a refusal. Refusing to start would turn a lost file
/// into a stopped agent, which is worse. Starting without saying so would be worse still, because
/// the agent would come up looking healthy and knowing nothing, and nobody would be told why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Memory {
    Kept,
    Missing,
}

/// How much of what an agent had is still with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Continuity {
    pub process: Process,
    pub session: Session,
    pub memory: Memory,
}

impl Continuity {
    /// What is true about a process about to be started for this record.
    ///
    /// `remembers` is whether the agent's summary is on disk, asked of the caller rather than of
    /// the filesystem here, so every case below can be written as a test without a folder.
    pub fn of(record: &Runtime, how: Start, remembers: bool) -> Self {
        Self {
            process: match record.lifecycle {
                Lifecycle::Never => Process::First,
                _ => Process::Replaced,
            },
            session: match how {
                Start::Fresh => Session::Fresh,
                Start::Resume => Session::Resumed,
                Start::Renew => Session::Replaced,
            },
            memory: match remembers {
                true => Memory::Kept,
                false => Memory::Missing,
            },
        }
    }

    /// Whether anything was lost getting here.
    ///
    /// A first process with a fresh conversation is not degraded. There was nothing to keep, so
    /// nothing failed to be kept, and calling a newly enlisted agent degraded would make the word
    /// mean nothing on the day it matters.
    pub fn degraded(&self) -> bool {
        self.memory == Memory::Missing
            || (self.process == Process::Replaced && self.session != Session::Resumed)
    }

    /// One line, for a person or a panel. Says what is wrong, or says nothing is.
    pub fn describe(&self) -> String {
        let mut said = match (self.process, self.session) {
            (Process::First, _) => "first process".to_string(),
            (Process::Replaced, Session::Resumed) => "restarted, conversation kept".into(),
            (Process::Replaced, Session::Replaced) => {
                "restarted, conversation could not be resumed".into()
            }
            (Process::Replaced, Session::Fresh) => "restarted, no conversation to resume".into(),
        };
        if self.memory == Memory::Missing {
            said.push_str(", and its memory folder is missing");
        }
        said
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::army::personnel::AgentId;

    fn record(lifecycle: Lifecycle) -> Runtime {
        let mut r = Runtime::never(AgentId::fresh().unwrap(), "nora", 0);
        r.lifecycle = lifecycle;
        r.session = Some(SessionId::fresh().unwrap());
        r
    }

    fn exited() -> Runtime {
        record(Lifecycle::Exited {
            code: Some(1),
            at: 10,
        })
    }

    #[test]
    fn a_first_process_with_a_new_conversation_has_lost_nothing() {
        let c = Continuity::of(&record(Lifecycle::Never), Start::Fresh, true);
        assert_eq!(c.process, Process::First);
        assert_eq!(c.session, Session::Fresh);
        assert!(!c.degraded(), "nothing existed to lose");
    }

    /// The case the whole design is for. The process died, and the agent does not notice.
    #[test]
    fn a_replaced_process_that_resumed_is_not_degraded() {
        let c = Continuity::of(&exited(), Start::Resume, true);
        assert_eq!(c.process, Process::Replaced);
        assert_eq!(c.session, Session::Resumed);
        assert!(!c.degraded());
        assert!(c.describe().contains("conversation kept"), "{c:?}");
    }

    /// The case that must never be reported as an ordinary restart, because the agent that comes
    /// back does not remember being asked anything.
    #[test]
    fn a_replaced_conversation_is_degraded_and_says_so() {
        let c = Continuity::of(&exited(), Start::Renew, true);
        assert_eq!(c.session, Session::Replaced);
        assert!(c.degraded());
        assert!(c.describe().contains("could not be resumed"), "{c:?}");
    }

    /// A conversation is not the only thing an agent has, and losing the other one is worse.
    #[test]
    fn a_missing_memory_folder_is_degraded_whatever_happened_to_the_conversation() {
        for how in [Start::Fresh, Start::Resume, Start::Renew] {
            let c = Continuity::of(&exited(), how, false);
            assert_eq!(c.memory, Memory::Missing);
            assert!(c.degraded(), "{how:?}");
            assert!(c.describe().contains("memory folder is missing"), "{c:?}");
        }
    }

    /// A process that was never started and one that was replaced are different, and the
    /// difference is what makes a fresh conversation innocent or alarming.
    #[test]
    fn the_same_conversation_state_reads_differently_on_a_first_and_a_later_process() {
        let first = Continuity::of(&record(Lifecycle::Never), Start::Fresh, true);
        let later = Continuity::of(&exited(), Start::Fresh, true);
        assert_eq!(first.session, later.session);
        assert!(!first.degraded());
        assert!(
            later.degraded(),
            "it had a process before and has no conversation"
        );
    }

    #[test]
    fn continuity_round_trips() {
        let c = Continuity::of(&exited(), Start::Renew, false);
        let text = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Continuity>(&text).unwrap(), c);
        assert!(text.contains("replaced"), "{text}");
    }
}
