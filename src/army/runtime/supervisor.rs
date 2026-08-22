//! The one thing that starts, watches and ends agent processes.
//!
//! Everything hard about this module is in `policy`, which is pure, and everything on disk is in
//! `store`, which is small. What is left here is the part that genuinely needs a process: spawning
//! one, noticing it has gone, and ending one that nothing can talk to any more.
//!
//! **One pass is `tick`.** No loop, no clock, no sleeping. A supervisor that owned its own timing
//! could only be tested by waiting, and the interesting states here take four crashes and a
//! restart to reach. The loop is in `main`, where a loop belongs, and a systemd unit will one day
//! be the thing that restarts the loop.
//!
//! **It hands out no work.** There is no way from here to give an agent a task, and that is not an
//! omission. Carl controls work and the supervisor controls process existence, so the only thing
//! this ever says to an agent is where its memory folder is.
//!
//! **It writes to the same journal Carl does, and that is the point.** The records under `run/`
//! answer what is true now. The journal answers what happened, and "the worker crashed, and then
//! the task was reported finished" is a sentence somebody has to be able to read in order. Two
//! files cannot be read in order, so there is one, and the numbering is locked so two writers
//! cannot claim one place in it.
//!
//! **The awkward case, written down because it looks like a bug.** When a supervisor exits, its
//! agents' processes go with it, because dropping a session closes stdin and waits. Their records
//! still say running. The next supervisor finds a record claiming a process that is not there,
//! writes down that it exited, and starts a new one resuming the same session. That is the design
//! working: the process was disposable, the conversation was not.

use std::path::PathBuf;
use std::time::Duration;

use super::continuity::{self, Continuity};
use super::lock::Lock;
use super::policy::{self, Next, Start};
use super::record::{Lifecycle, Runtime};
use super::store::Roll;
use crate::army::chain::{brief_for, tools_for};
use crate::army::event::{Because, Event, Journal};
use crate::army::personnel::{AgentId, Personnel, memory};
use crate::claude::{Runner, Session};
use crate::providers::system::started;
use crate::{Result, SessionId};

/// Who the runtime events are attributed to.
///
/// Not an agent, and deliberately not the name of the agent a record is about. A process ending
/// is something that happened to an agent rather than something it did, and writing it down as
/// though the agent did it would make "the last thing Nora did" answer with Nora dying.
pub const ACTOR: &str = "supervisor";

/// How long a reclaimed process is given to end politely before it is not asked again.
const POLITE: std::time::Duration = std::time::Duration::from_secs(2);

/// What happened to one agent in one pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Running, and ours.
    Left,
    Started(Start),
    /// A process from a previous supervisor was ended and replaced.
    Reclaimed,
    Waiting {
        until: u64,
    },
    /// The supervisor has stopped trying, or was already not trying.
    NotStarting {
        why: String,
    },
    /// Nothing could be done about this agent, and it is not the agent's fault.
    Skipped {
        why: String,
    },
    /// A start was attempted and did not happen.
    Failed {
        why: String,
    },
}

/// One pass over the army.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tick {
    pub at: u64,
    pub what: Vec<(String, Outcome)>,
}

impl Tick {
    pub fn count(&self, of: impl Fn(&Outcome) -> bool) -> usize {
        self.what.iter().filter(|(_, o)| of(o)).count()
    }

    /// One line per agent, for a person watching a terminal.
    pub fn lines(&self) -> Vec<String> {
        self.what
            .iter()
            .map(|(name, outcome)| {
                let said = match outcome {
                    Outcome::Left => "running".to_string(),
                    Outcome::Started(Start::Fresh) => "started, new conversation".into(),
                    Outcome::Started(Start::Resume) => "started, resuming".into(),
                    Outcome::Started(Start::Renew) => {
                        "started, the old session was set aside".into()
                    }
                    Outcome::Reclaimed => "an orphan was ended and replaced".into(),
                    Outcome::Waiting { until } => format!("waiting until {until}"),
                    Outcome::NotStarting { why } => format!("not being started: {why}"),
                    Outcome::Skipped { why } => format!("skipped: {why}"),
                    Outcome::Failed { why } => format!("could not start: {why}"),
                };
                format!("  {name:8} {said}")
            })
            .collect()
    }
}

/// The army's processes.
pub struct Supervisor {
    home: PathBuf,
    /// Where the `claude` binary is, so a test can point at a stand in.
    program: PathBuf,
    roll: Roll,
    /// The sessions this supervisor owns, which is what a record cannot hold.
    ///
    /// A record can say a process is running. It cannot hold the pipes to it, and pipes are what
    /// ownership actually means here: a process whose pipes are gone is alive and unreachable.
    live: Vec<(AgentId, Session)>,
    /// Where what happened is written down. The same file Carl writes work to.
    journal: Journal,
    pid: u32,
    /// Dropped last, releasing the claim on this home.
    _lock: Lock,
}

impl Supervisor {
    /// Claims a home. Refuses if another supervisor already has it.
    pub fn take(home: impl Into<PathBuf>, program: impl Into<PathBuf>) -> Result<Self> {
        let home = home.into();
        let _lock = Lock::take(&home)?;
        Ok(Self {
            roll: Roll::open(&home)?,
            journal: Journal::open(home.join("run").join("events.jsonl"))?,
            home,
            program: program.into(),
            live: Vec::new(),
            pid: std::process::id(),
            _lock,
        })
    }

    pub fn roll(&self) -> &Roll {
        &self.roll
    }

    /// How many processes this supervisor is holding open.
    pub fn holding(&self) -> usize {
        self.live.len()
    }

    /// One pass over every agent that has a folder.
    pub fn tick(&mut self, people: &Personnel, now: u64) -> Result<Tick> {
        let mut what = Vec::new();
        for name in people.names() {
            what.push((name.to_string(), self.one(people, name, now)?));
        }
        Ok(Tick { at: now, what })
    }

    /// Deliberately does not start this agent again until somebody says otherwise.
    ///
    /// The supervisor's half of a stop. It says nothing about the agent's work, which is not the
    /// supervisor's to say, only that no process is to be kept running for it.
    pub fn stop(&mut self, agent: &AgentId, why: impl Into<String>, now: u64) -> Result<()> {
        let Some(mut record) = self.roll.get(agent).cloned() else {
            return Ok(());
        };
        let why = why.into();
        self.live.retain(|(id, _)| id != agent);
        record.lifecycle = Lifecycle::Stopped { why: why.clone() };
        record.supervisor = None;
        record.updated_at = now;

        self.journal.append(
            ACTOR,
            Event::AgentStopped {
                agent: record.agent.clone(),
                name: record.name.clone(),
                why,
            },
        )?;
        self.roll.save(&self.home, record)
    }

    /// Asks for a stopped or sleeping agent again, saying what for.
    ///
    /// The supervisor's half of a wake. It clears the refusal so the next pass starts a process,
    /// and it has no opinion about whether the reason is a good one, because judging that is
    /// work and work is Carl's.
    ///
    /// The reason is a value with no "in general" in it. An agent woken for nothing in
    /// particular is an agent nobody can say why is running, and what that costs is a model
    /// sitting there thinking.
    ///
    /// Returns whether anything was actually done. An agent that is already up needs nothing,
    /// and this must not touch it: clearing the lifecycle of a running agent makes the next pass
    /// see a record with no process behind it, start a second one resuming the same
    /// conversation, and drop the first, which closes the pipe the caller was about to use. That
    /// is not theoretical. It is what happened the first time this was pointed at real sessions,
    /// and it looked like the model failing to answer.
    pub fn wake(&mut self, agent: &AgentId, because: Because, now: u64) -> Result<bool> {
        let Some(mut record) = self.roll.get(agent).cloned() else {
            return Err(crate::Error::Refused(format!(
                "{agent} has no runtime record, so there is nothing asleep to wake"
            )));
        };

        // Already up, and held by this supervisor, so there is nothing to wake. No event
        // either: a wake nobody performed is not something that happened.
        if let Lifecycle::Running { pid, started, .. } = record.lifecycle
            && record.owned_by(self.pid)
            && started::is_still(pid, started)
        {
            return Ok(false);
        }

        // Degraded is not asleep. It means starting this agent is something the supervisor
        // could not fix by doing it again, and clearing that here would restart the loop that
        // gave up in the first place.
        if let Lifecycle::Degraded { why } = &record.lifecycle {
            return Err(crate::Error::Refused(format!(
                "{} was given up on, not put to sleep: {why}. Waking it would start the same \
                 loop again.",
                record.name
            )));
        }

        self.journal.append(
            ACTOR,
            Event::AgentWoken {
                agent: agent.clone(),
                name: record.name.clone(),
                because,
            },
        )?;

        // Back to the state a process that ended leaves behind, so the ordinary policy decides
        // what happens next. Starting it here would be the supervisor keeping a second way to
        // start an agent, and two ways to do one thing is two backoff counters.
        record.lifecycle = Lifecycle::Exited {
            code: None,
            at: now,
        };
        record.attempts = 0;
        record.supervisor = None;
        record.updated_at = now;
        self.roll.save(&self.home, record)?;
        Ok(true)
    }

    /// Hands a message to an agent's process and returns what it said.
    ///
    /// Delivery, not instruction. The supervisor never composes a message and has nowhere to
    /// get one from; it holds the pipe, and holding the pipe is the only reason this is here
    /// rather than in Carl. What is in the message, who it is for and whether it should be sent
    /// at all are decided by whoever calls this.
    ///
    /// Refused for an agent this supervisor is not holding, including one that is running under
    /// a supervisor that has since gone, because that process is alive and unreachable.
    pub fn deliver(&mut self, agent: &AgentId, text: &str, deadline: Duration) -> Result<String> {
        let session = self
            .live
            .iter_mut()
            .find(|(id, _)| id == agent)
            .map(|(_, session)| session)
            .ok_or_else(|| {
                crate::Error::Refused(format!(
                    "{agent} has no process this supervisor is holding, so there is nothing to \
                     say it to"
                ))
            })?;

        let began = std::time::Instant::now();
        let answer = session.ask(
            text,
            &mut |_| crate::claude::Flow::Continue,
            &mut || match began.elapsed() > deadline {
                true => crate::claude::Flow::Stop,
                false => crate::claude::Flow::Continue,
            },
        )?;

        match answer.interrupted {
            true => Err(crate::Error::Claude(format!(
                "{agent} did not finish answering within {:.0}s",
                deadline.as_secs_f32()
            ))),
            false => Ok(answer.text),
        }
    }

    fn one(&mut self, people: &Personnel, name: &str, now: u64) -> Result<Outcome> {
        let folder = people.get(name).expect("named by the roster");
        let Some(identity) = folder.identity.as_ref() else {
            return Ok(Outcome::Skipped {
                why: "no identity, so there is nothing durable to attach a process to".into(),
            });
        };
        let id = identity.id.clone();

        let mut record = match self.roll.get(&id) {
            Some(known) => known.clone(),
            None => Runtime::never(id.clone(), name, now),
        };
        let alive = self.reconcile(&mut record, now)?;

        match policy::decide(&record, self.pid, alive, now) {
            Next::Leave => Ok(Outcome::Left),
            Next::Wait { until } => Ok(Outcome::Waiting { until }),
            Next::Refuse { why } => Ok(Outcome::NotStarting { why }),
            Next::GiveUp { why } => {
                record.lifecycle = Lifecycle::Degraded { why: why.clone() };
                record.supervisor = None;
                record.updated_at = now;
                self.journal.append(
                    ACTOR,
                    Event::AgentGaveUp {
                        agent: record.agent.clone(),
                        name: record.name.clone(),
                        why: why.clone(),
                    },
                )?;
                self.roll.save(&self.home, record)?;
                Ok(Outcome::NotStarting { why })
            }
            Next::Reclaim { pid, started } => {
                end(pid, started);
                Ok(
                    match self.start(people, name, record, Start::Resume, now)? {
                        Outcome::Started(_) => Outcome::Reclaimed,
                        other => other,
                    },
                )
            }
            Next::Start(how) => self.start(people, name, record, how, now),
        }
    }

    /// Brings the record into line with what is actually true, and says whether it is running.
    ///
    /// The only place an exit is counted, so a process that both failed to spawn and was later
    /// observed missing cannot be counted twice.
    fn reconcile(&mut self, record: &mut Runtime, now: u64) -> Result<bool> {
        let Lifecycle::Running {
            pid,
            started,
            since,
        } = record.lifecycle
        else {
            return Ok(false);
        };
        if started::is_still(pid, started) {
            return Ok(true);
        }

        // Ours, so the code is available and worth keeping. Somebody else's, and all that is
        // known is that it has gone.
        let code = self
            .live
            .iter_mut()
            .find(|(id, _)| *id == record.agent)
            .and_then(|(_, session)| session.ended())
            .flatten();
        self.live.retain(|(id, _)| *id != record.agent);

        record.lifecycle = Lifecycle::Exited { code, at: now };
        record.supervisor = None;
        record.updated_at = now;
        // A process that stayed up did not fail to start, whatever ended it.
        record.attempts = match policy::was_healthy(since, now) {
            true => 0,
            false => record.attempts.saturating_add(1),
        };

        // Written before the record is saved. A crash after the write leaves an outcome that can
        // be looked up. A crash before it loses the only evidence anything happened at all.
        self.journal.append(
            ACTOR,
            Event::AgentCrashed {
                agent: record.agent.clone(),
                name: record.name.clone(),
                code,
                attempt: record.attempts,
            },
        )?;
        self.roll.save(&self.home, record.clone())?;
        Ok(false)
    }

    fn start(
        &mut self,
        people: &Personnel,
        name: &str,
        mut record: Runtime,
        how: Start,
        now: u64,
    ) -> Result<Outcome> {
        let folder = people.get(name).expect("named by the roster");

        // Setting a session aside is a change to the record whether or not the start works, so
        // it happens first and the id is kept rather than dropped.
        if how == Start::Renew
            && let Some(old) = record.session.take()
        {
            self.journal.append(
                ACTOR,
                Event::ContinuityChanged {
                    agent: record.agent.clone(),
                    name: name.to_string(),
                    from: continuity::Session::Resumed,
                    to: continuity::Session::Replaced,
                    why: format!(
                        "{} starts in a row did not stick while resuming, so the conversation is \
                         the suspect and retrying it cannot fix a transcript that is too long, \
                         corrupt or gone",
                        record.attempts
                    ),
                    abandoned: Some(old.clone()),
                },
            )?;
            record.abandoned.push(old);
        }

        let session = match (how, &record.session) {
            (Start::Resume, Some(existing)) => existing.clone(),
            _ => SessionId::fresh()?,
        };
        let resume = how == Start::Resume && record.session.is_some();

        let workdir = people.folder(name);
        // Asked before the process starts, because after it there is nothing to do about the
        // answer, and an agent that comes up looking healthy while knowing nothing is the one
        // failure here that reports itself as a success.
        let continuity = Continuity::of(&record, how, memory::summary_path(&workdir).is_file());
        let system = format!(
            "{}\n\n{}",
            brief_for(folder.agent),
            memory::embedded_fact(&workdir)
        );
        let runner = Runner::at(&self.program).allowing(tools_for(folder.agent.rank));

        record.name = name.to_string();
        record.session = Some(session.clone());
        record.continuity = Some(continuity);
        record.updated_at = now;

        // Two ways for this to fail and they are the same failure: no process. Either the binary
        // will not run, or it ran and was gone before its own start time could be read, which is
        // a process that exited immediately.
        let outcome = runner
            .open_session(&session, &workdir, &system, resume)
            .and_then(|open| match started::started(open.pid()) {
                Some(started) => Ok((open, started)),
                None => Err(crate::Error::Claude(
                    "the process was gone before it could be recorded".into(),
                )),
            });

        match outcome {
            Ok((open, started)) => {
                record.lifecycle = Lifecycle::Running {
                    pid: open.pid(),
                    started,
                    since: now,
                };
                record.supervisor = Some(self.pid);
                let id = record.agent.clone();
                self.journal.append(
                    ACTOR,
                    Event::AgentStarted {
                        agent: id.clone(),
                        name: name.to_string(),
                        continuity,
                        attempt: record.attempts,
                    },
                )?;
                self.roll.save(&self.home, record)?;

                // Replacing rather than adding. Anything still held for this agent is a session
                // whose process has gone, and dropping it here is what finally reaps it.
                self.live.retain(|(held, _)| *held != id);
                self.live.push((id, open));
                Ok(Outcome::Started(how))
            }
            Err(e) => {
                record.lifecycle = Lifecycle::Exited {
                    code: None,
                    at: now,
                };
                record.supervisor = None;
                record.attempts = record.attempts.saturating_add(1);
                self.journal.append(
                    ACTOR,
                    Event::AgentStartFailed {
                        agent: record.agent.clone(),
                        name: name.to_string(),
                        why: e.to_string(),
                        attempt: record.attempts,
                    },
                )?;
                self.roll.save(&self.home, record)?;
                Ok(Outcome::Failed { why: e.to_string() })
            }
        }
    }
}

/// Ends a process this supervisor cannot talk to.
///
/// Politely first, because a `claude` process asked to stop writes the end of its transcript, and
/// the transcript is what the replacement is about to resume. Checked immediately before every
/// signal, because a pid whose process has just exited is a pid that may already belong to
/// somebody else's editor.
fn end(pid: u32, started: u64) {
    if !started::is_still(pid, started) {
        return;
    }
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };

    let deadline = std::time::Instant::now() + POLITE;
    while std::time::Instant::now() < deadline {
        if !started::is_still(pid, started) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    if started::is_still(pid, started) {
        unsafe { libc::kill(pid as libc::pid_t, libc::SIGKILL) };
    }
}
