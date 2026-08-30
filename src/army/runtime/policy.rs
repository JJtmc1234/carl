//! What to do next about one agent, worked out from a record and nothing else.
//!
//! Pure on purpose. Deciding is separated from doing because every interesting case here is one
//! nobody wants to reproduce by hand: a process that has crashed four times, a supervisor that
//! was itself restarted, a session that will not resume. Against a function taking a record and
//! returning a decision, all of those are three lines of a test. Against a supervisor that spawns
//! things, none of them are.
//!
//! **The policy JJ asked for.** Immediate on the first crash, then backoff, then degraded session
//! recovery. Each of the three is a different claim about what is wrong.
//!
//! - Once is bad luck. Something killed it, the machine was busy, it lost a race. Try again now,
//!   because waiting five seconds to find out helps nobody.
//! - Repeatedly is a fault, and hammering a fault turns one broken agent into a busy loop that
//!   makes the whole machine worse. So the gap doubles.
//! - Repeatedly while resuming is a different fault, and it points at the session. A transcript
//!   can be too long, corrupt, or gone. Retrying the resume cannot fix any of those, so the
//!   session is set aside and a fresh one is started under the same agent id.
//!
//! **The last of those is why the embedded memory fact exists**, and it is worth saying plainly
//! because it looked like two unrelated decisions until it did not. A fresh session is not a
//! reset agent. Every agent is told, permanently and for free, that its memory folder is there
//! and that `summary.md` is the way in. So a session started from nothing reads what the agent
//! knew, without the supervisor having to seed it with anything, and without the supervisor
//! having to have an opinion about what an agent should remember. The supervisor stays a thing
//! that owns processes.
//!
//! **And it may refuse.** After enough failures with a fresh session too, there is nothing left
//! to vary, and continuing would be a loop with a cost. The record says degraded and why, the
//! panel shows it, and a person decides. A supervisor that cannot give up is a supervisor whose
//! failure mode is spending money.

use super::record::{Lifecycle, Runtime};

/// How long the first backoff is. Doubles from here.
pub const BACKOFF_BASE: u64 = 5;

/// The longest gap between attempts. Five minutes, so a permanently broken agent costs one
/// wasted start every five minutes rather than one every five seconds.
pub const BACKOFF_MAX: u64 = 300;

/// How many failures before the session itself is treated as the suspect.
pub const RENEW_AFTER: u32 = 3;

/// How many failures before the supervisor stops trying at all.
///
/// Above `RENEW_AFTER`, so a fresh session genuinely gets its own attempts rather than inheriting
/// a count that has already nearly run out.
pub const GIVE_UP_AFTER: u32 = 6;

/// How long a process has to stay up before its exit counts as bad luck rather than a fault.
///
/// Without this, an agent restarted once a night accumulates attempts for a fortnight and then
/// declares itself degraded, having never actually failed to start.
pub const HEALTHY_FOR: u64 = 60;

/// Which conversation a new process should be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// No session yet. Pin a new one.
    Fresh,
    /// Continue the recorded session, which is the normal case and the point of the design.
    Resume,
    /// The recorded session is the suspect. Set it aside, keep the id, pin a new one.
    Renew,
}

/// What the supervisor should do about one agent right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Running, and ours. The commonest answer and the one that costs nothing.
    Leave,
    Start(Start),
    /// Alive, and started by a supervisor that is gone.
    ///
    /// Its pipes died with its parent, so nothing can talk to it. It cannot be adopted, only
    /// ended and replaced by a process resuming the same session.
    Reclaim {
        pid: u32,
        started: u64,
    },
    /// In backoff. `until` is unix seconds.
    Wait {
        until: u64,
    },
    /// Stop trying, and record why.
    GiveUp {
        why: String,
    },
    /// Somebody has already decided this agent is not to be started.
    Refuse {
        why: String,
    },
}

/// How long to wait after `attempts` consecutive failures before trying again.
///
/// Zero after the first, because once is bad luck and waiting five seconds to find that out
/// helps nobody. The count is of failures that have already happened, so the free one is
/// `attempts == 1` rather than `attempts == 0`. Getting that off by one wrong is not a small
/// slip: it means the first restart waits, which is the one case the policy says must not.
pub fn backoff(attempts: u32) -> u64 {
    match attempts {
        0 | 1 => 0,
        n => BACKOFF_BASE
            .saturating_mul(1u64 << (n - 2).min(20))
            .min(BACKOFF_MAX),
    }
}

/// What to do about this agent, given who is asking and what time it is.
///
/// `alive` is the answer to "is the process this record names still the one it named", supplied
/// by the caller rather than read here, so that every case below is testable without a process.
/// It is ignored unless the record claims something is running.
pub fn decide(record: &Runtime, supervisor: u32, alive: bool, now: u64) -> Next {
    match &record.lifecycle {
        Lifecycle::Stopped { why } | Lifecycle::Degraded { why } => {
            Next::Refuse { why: why.clone() }
        }

        Lifecycle::Never => Next::Start(Start::Fresh),

        // Refused rather than waited for, and the difference matters. A wait is a deadline this
        // module worked out and will act on. Sleep ends when the timetable says so, which is
        // somewhere else entirely, and a deadline invented here would be a second opinion about
        // when the morning is.
        Lifecycle::Asleep { .. } => Next::Refuse {
            why: "asleep, by its own hours".into(),
        },

        Lifecycle::Running { pid, started, .. } => match (alive, record.owned_by(supervisor)) {
            (true, true) => Next::Leave,
            (true, false) => Next::Reclaim {
                pid: *pid,
                started: *started,
            },
            // The record is stale. Nothing to reclaim and nothing to wait for, because the
            // exit was never observed and so was never counted. Whoever writes the outcome
            // counts it; this only says to get the agent running again.
            (false, _) => Next::Start(resume_or_fresh(record)),
        },

        Lifecycle::Exited { at, .. } => {
            if record.attempts >= GIVE_UP_AFTER {
                return Next::GiveUp {
                    why: format!(
                        "{} starts in a row did not stick, including with a fresh session",
                        record.attempts
                    ),
                };
            }
            let until = at.saturating_add(backoff(record.attempts));
            if now < until {
                return Next::Wait { until };
            }
            // Equality rather than "at least", and it matters. A session is set aside once
            // per streak of failures. With `>=` every attempt after the third would abandon
            // another session, and the fresh one would never get the attempts it is owed.
            if record.attempts == RENEW_AFTER && record.session.is_some() {
                return Next::Start(Start::Renew);
            }
            Next::Start(resume_or_fresh(record))
        }
    }
}

/// Resume only a conversation that is known to exist.
///
/// A recorded id is an intention. An established one is a conversation a process actually got
/// far enough to create. Resuming the first fails immediately and forever, because `--resume`
/// on an id claude never wrote is an error rather than a fresh start, and the supervisor counts
/// it as another failed start.
fn resume_or_fresh(record: &Runtime) -> Start {
    match record.session.is_some() && record.established {
        true => Start::Resume,
        false => Start::Fresh,
    }
}

/// Whether an exit that has just been observed should clear the attempt count.
///
/// A process that stayed up and then ended was not a failure to start, whatever ended it.
pub fn was_healthy(since: u64, until: u64) -> bool {
    until.saturating_sub(since) >= HEALTHY_FOR
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SessionId;
    use crate::army::personnel::AgentId;

    fn record() -> Runtime {
        Runtime::never(AgentId::fresh().unwrap(), "nora", 0)
    }

    /// A session here is an established one, because that is what these tests are about: what
    /// happens to an agent whose conversation exists. The case where it does not is its own
    /// test, and it is the one that took the whole army down.
    fn exited(attempts: u32, at: u64, with_session: bool) -> Runtime {
        let mut r = record();
        r.lifecycle = Lifecycle::Exited { code: Some(1), at };
        r.attempts = attempts;
        if with_session {
            r.session = Some(SessionId::fresh().unwrap());
            r.established = true;
        }
        r
    }

    /// A recorded id that no process ever got far enough to create.
    fn exited_with_a_session_that_never_existed(attempts: u32, at: u64) -> Runtime {
        let mut r = exited(attempts, at, true);
        r.established = false;
        r
    }

    /// A record that claims a process carries an established session, because a process that
    /// got as far as being recorded as running is one that created its conversation.
    fn running(supervisor: Option<u32>) -> Runtime {
        let mut r = record();
        r.session = Some(SessionId::fresh().unwrap());
        r.established = true;
        r.lifecycle = Lifecycle::Running {
            pid: 4321,
            started: 987,
            since: 10,
        };
        r.session = Some(SessionId::fresh().unwrap());
        r.supervisor = supervisor;
        r
    }

    #[test]
    fn an_agent_that_has_never_run_starts_a_new_conversation() {
        assert_eq!(decide(&record(), 1, false, 0), Next::Start(Start::Fresh));
    }

    #[test]
    fn a_process_we_started_and_that_is_still_there_is_left_alone() {
        assert_eq!(decide(&running(Some(7)), 7, true, 100), Next::Leave);
    }

    /// The case that only happens when the supervisor itself is restarted, and the one most
    /// likely to be got wrong, because the process is perfectly healthy and completely useless.
    #[test]
    fn a_live_process_from_a_dead_supervisor_is_reclaimed_rather_than_adopted() {
        assert_eq!(
            decide(&running(Some(6)), 7, true, 100),
            Next::Reclaim {
                pid: 4321,
                started: 987
            }
        );
    }

    /// A record claiming a process that is gone is a record that was never updated, which is
    /// what a supervisor killed mid write leaves behind.
    #[test]
    fn a_stale_running_record_starts_again_and_resumes() {
        assert_eq!(
            decide(&running(Some(7)), 7, false, 100),
            Next::Start(Start::Resume)
        );
    }

    /// Once is bad luck. Waiting to find that out helps nobody.
    /// One failure has happened by the time anybody asks, so the free restart is at one and not
    /// at zero. An off by one here means the first crash waits, which is the one thing the
    /// policy says it must not do.
    #[test]
    fn the_first_crash_is_restarted_immediately() {
        assert_eq!(backoff(1), 0, "one failure, and it was free");
        assert_eq!(
            decide(&exited(1, 100, true), 1, false, 100),
            Next::Start(Start::Resume)
        );
    }

    #[test]
    fn later_crashes_wait_longer_each_time_and_then_stop_growing() {
        assert_eq!(backoff(2), 5);
        assert_eq!(backoff(3), 10);
        assert_eq!(backoff(4), 20);
        assert_eq!(backoff(30), BACKOFF_MAX, "capped, and does not overflow");
        assert!(
            (1..30).all(|n| backoff(n) <= backoff(n + 1)),
            "never goes backwards"
        );
    }

    #[test]
    fn an_agent_in_backoff_is_waited_for_and_then_started() {
        let r = exited(2, 100, true);
        assert_eq!(decide(&r, 1, false, 101), Next::Wait { until: 105 });
        assert_eq!(decide(&r, 1, false, 105), Next::Start(Start::Resume));
    }

    /// Retrying a resume cannot fix a transcript that is too long, corrupt or gone. The only
    /// thing left to vary is the session, so it is varied, once, and kept.
    #[test]
    fn enough_failed_resumes_set_the_session_aside_rather_than_trying_it_again() {
        let r = exited(RENEW_AFTER, 0, true);
        assert_eq!(decide(&r, 1, false, 10_000), Next::Start(Start::Renew));
    }

    /// A streak of failures sets one session aside, not one per attempt. Otherwise the fresh
    /// session never gets the attempts it is owed and every one of them is thrown away.
    #[test]
    fn a_session_is_set_aside_once_and_the_fresh_one_is_then_resumed() {
        let mut r = exited(RENEW_AFTER + 1, 0, true);
        assert_eq!(decide(&r, 1, false, 10_000), Next::Start(Start::Resume));
        r.attempts = GIVE_UP_AFTER - 1;
        assert_eq!(decide(&r, 1, false, 10_000), Next::Start(Start::Resume));
    }

    /// Renewing is only meaningful when there is a session to blame. Without one there is
    /// nothing to set aside and the next start is simply a fresh one.
    #[test]
    fn there_is_nothing_to_renew_when_no_session_was_ever_pinned() {
        let r = exited(RENEW_AFTER, 0, false);
        assert_eq!(decide(&r, 1, false, 10_000), Next::Start(Start::Fresh));
    }

    /// A supervisor that cannot give up has a failure mode that costs money every five minutes
    /// forever, and shows a healthy looking agent doing nothing.
    #[test]
    fn an_agent_that_fails_even_with_a_fresh_session_is_given_up_on_and_says_why() {
        let r = exited(GIVE_UP_AFTER, 0, true);
        let Next::GiveUp { why } = decide(&r, 1, false, 10_000) else {
            panic!("should have given up");
        };
        assert!(why.contains("fresh session"), "{why}");
    }

    #[test]
    fn a_degraded_or_stopped_agent_is_refused_with_the_reason_it_was_given() {
        for lifecycle in [
            Lifecycle::Degraded {
                why: "keeps dying".into(),
            },
            Lifecycle::Stopped {
                why: "JJ said so".into(),
            },
        ] {
            let mut r = record();
            let expected = match &lifecycle {
                Lifecycle::Degraded { why } | Lifecycle::Stopped { why } => why.clone(),
                _ => unreachable!(),
            };
            r.lifecycle = lifecycle;
            assert_eq!(decide(&r, 1, false, 10_000), Next::Refuse { why: expected });
        }
    }

    /// Without this an agent restarted once a night is degraded within a fortnight, having
    /// never once failed to start.
    #[test]
    fn a_process_that_stayed_up_did_not_fail_to_start() {
        assert!(was_healthy(0, HEALTHY_FOR));
        assert!(!was_healthy(0, HEALTHY_FOR - 1));
        assert!(
            !was_healthy(100, 50),
            "a clock that went backwards is not up"
        );
    }

    /// The bug that took all ten agents down for twenty one hours on 2026 08 28.
    ///
    /// Renewing at three failures mints a fresh id and stores it. If that process dies before
    /// writing anything, the id names no conversation, and `--resume` on it is an error rather
    /// than a fresh start. Every later attempt then fails identically until the supervisor gives
    /// up, and the record is left naming a conversation that has never existed.
    #[test]
    fn a_session_no_process_ever_created_is_never_resumed() {
        for attempts in [1, 2, 4, 5] {
            let record = exited_with_a_session_that_never_existed(attempts, 0);
            let decided = decide(&record, 1, false, now_well_past_any_backoff());
            assert_eq!(
                decided,
                Next::Start(Start::Fresh),
                "attempt {attempts} resumed a conversation that was never created"
            );
        }
    }

    /// And the other half, or the fix would just throw away every conversation. A session a
    /// process actually lived on is resumed as before.
    #[test]
    fn a_session_a_process_lived_on_is_still_resumed() {
        let record = exited(1, 0, true);
        assert_eq!(
            decide(&record, 1, false, now_well_past_any_backoff()),
            Next::Start(Start::Resume)
        );
    }

    /// Far enough past any backoff that the decision under test is the session, not the wait.
    fn now_well_past_any_backoff() -> u64 {
        BACKOFF_MAX * 4
    }
}
