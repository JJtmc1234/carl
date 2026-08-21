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
//! **The awkward case, written down because it looks like a bug.** When a supervisor exits, its
//! agents' processes go with it, because dropping a session closes stdin and waits. Their records
//! still say running. The next supervisor finds a record claiming a process that is not there,
//! writes down that it exited, and starts a new one resuming the same session. That is the design
//! working: the process was disposable, the conversation was not.

use std::path::PathBuf;

use super::lock::Lock;
use super::policy::{self, Next, Start};
use super::record::{Lifecycle, Runtime};
use super::store::Roll;
use crate::army::chain::{brief_for, tools_for};
use crate::army::personnel::{AgentId, Personnel, memory};
use crate::claude::{Runner, Session};
use crate::providers::system::started;
use crate::{Result, SessionId};

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
        self.live.retain(|(id, _)| id != agent);
        record.lifecycle = Lifecycle::Stopped { why: why.into() };
        record.supervisor = None;
        record.updated_at = now;
        self.roll.save(&self.home, record)
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
            record.abandoned.push(old);
        }

        let session = match (how, &record.session) {
            (Start::Resume, Some(existing)) => existing.clone(),
            _ => SessionId::fresh()?,
        };
        let resume = how == Start::Resume && record.session.is_some();

        let workdir = people.folder(name);
        let system = format!(
            "{}\n\n{}",
            brief_for(folder.agent),
            memory::embedded_fact(&workdir)
        );
        let runner = Runner::at(&self.program).allowing(tools_for(folder.agent.rank));

        record.name = name.to_string();
        record.session = Some(session.clone());
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
