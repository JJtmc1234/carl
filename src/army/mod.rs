//! Many agents at once, and the harness that keeps them honest.
//!
//! One Carl answering one question is a conversation. Twenty agents building something is a
//! different machine, and almost everything hard about it is the harness rather than the
//! agents.
//!
//! The agents are the easy part. Each is a `claude` process with a brief, given one job, and
//! told to answer and stop. What decides whether the whole thing works is everything around
//! them.
//!
//! **Bounded concurrency.** Twenty processes is four gigabytes and twenty simultaneous model
//! calls. The machine can take it and the rate limit may not, so how many run at once is a
//! number rather than however many tasks there happen to be.
//!
//! **Nothing takes the run down.** A worker that fails, hangs, or answers with nothing is one
//! bad report among many. Nineteen useful answers and one failure is a result. Nineteen useful
//! answers thrown away because the twentieth timed out is not.
//!
//! **A deadline each.** An agent with no time limit is an agent that can wait forever, and one
//! of those holds up everything waiting on it.
//!
//! **Order that does not depend on timing.** Reports come back in whatever order they finish,
//! and are returned in the order they were asked for, because a run that reads differently
//! every time cannot be compared with the last one.
//!
//! **Everything is written down.** What each agent was asked, what it said, how long it took,
//! and whether it failed. A run nobody can inspect afterwards is a run nobody can fix.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use crate::claude::Runner;
use crate::{Result, SessionId};

mod campaign;
pub mod chain;
pub mod event;
pub mod org;
mod report;
mod roster;
pub mod task;

pub use campaign::{Departmental, JOBS_PER_HEAD, Outcome, parse_jobs, run_department};
pub use chain::{Chain, Passage, brief_for as chain_brief, campaign, tools_for};
pub use event::{Event, Journal, Record};
pub use org::{Agent, Rank, check_delegation, check_may_implement, may_delegate};
pub use report::{Report, Summary};
pub use roster::{Role, Roster, brief_for, department, heads, roles};
pub use task::{MAX_ATTEMPTS, Status, Task, TaskId, Verification, may_take_on};

/// How many agents may be working at once.
///
/// Not however many tasks there are. Twenty `claude` processes is about four gigabytes and
/// twenty simultaneous model calls, and the thing that gives way first is usually the rate
/// limit rather than the machine.
pub const AT_ONCE: usize = 6;

/// How long one agent may take before it is abandoned.
///
/// Generous, because some jobs really do take a while, and finite, because an agent with no
/// deadline holds up everything waiting on it.
pub const DEADLINE: Duration = Duration::from_secs(300);

/// One instruction sent to one agent, and the unit the concurrency runner works in.
///
/// Not a `Task`. A task is a governed thing with a goal, an owner, verification and a status,
/// and it may take several dispatches to finish one. Calling both of them Task was the first
/// thing that had to go, because two types with one name is how two people build two
/// incompatible halves of the same system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    /// Which role does it. Must name something in the roster.
    pub role: String,
    /// What this one agent is being asked for, on its own, with no reference to the others.
    pub instruction: String,
}

impl Dispatch {
    pub fn new(role: impl Into<String>, instruction: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            instruction: instruction.into(),
        }
    }
}

pub struct Army {
    runner: Runner,
    workdir: std::path::PathBuf,
    at_once: usize,
    deadline: Duration,
}

impl Army {
    /// Another army over the same binary and directory, with its own deadline.
    ///
    /// Needed because a head is asked one question at a time and should not wait as long as a
    /// worker chewing through a whole job.
    pub fn clone_for(&self, deadline: Duration) -> Self {
        Self {
            runner: Runner::at(self.runner.program()),
            workdir: self.workdir.clone(),
            at_once: 1,
            deadline,
        }
    }

    pub fn new(runner: Runner, workdir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            runner,
            workdir: workdir.into(),
            at_once: AT_ONCE,
            deadline: DEADLINE,
        }
    }

    pub fn at_once(mut self, n: usize) -> Self {
        self.at_once = n.max(1);
        self
    }

    pub fn deadline(mut self, d: Duration) -> Self {
        self.deadline = d;
        self
    }

    /// Runs every task, at most `at_once` at a time, and returns a report for each.
    ///
    /// Always one report per task, in the order the tasks were given. A failure is a report
    /// that says so rather than a missing entry, because a caller counting answers should not
    /// have to work out which one is absent.
    pub fn deploy(&self, tasks: &[Dispatch]) -> Vec<Report> {
        if tasks.is_empty() {
            return Vec::new();
        }

        let (done_tx, done_rx) = channel::<(usize, Report)>();
        let mut running = 0usize;
        let mut next = 0usize;
        let mut collected: Vec<Option<Report>> = (0..tasks.len()).map(|_| None).collect();

        std::thread::scope(|scope| {
            while collected.iter().any(Option::is_none) {
                // Fill the slots, then wait for one to come free. The alternative, launching
                // everything and hoping, is how twenty model calls arrive at a rate limit
                // together and nineteen of them fail.
                while running < self.at_once && next < tasks.len() {
                    let task = tasks[next].clone();
                    let at = next;
                    let tx = done_tx.clone();
                    let runner = &self.runner;
                    let workdir = self.workdir.clone();
                    let deadline = self.deadline;

                    scope.spawn(move || {
                        let report = work(runner, &workdir, &task, deadline);
                        // Nothing is done about a send failure. It means the collector has
                        // gone, which means the whole run is over.
                        let _ = tx.send((at, report));
                    });

                    running += 1;
                    next += 1;
                }

                match done_rx.recv() {
                    Ok((at, report)) => {
                        collected[at] = Some(report);
                        running -= 1;
                    }
                    // Every sender is gone and something is still missing. Cannot happen while
                    // this thread holds one, and breaking beats looping forever if it does.
                    Err(_) => break,
                }
            }
        });

        collected
            .into_iter()
            .enumerate()
            .map(|(at, r)| {
                r.unwrap_or_else(|| Report::failed(&tasks[at].role, "no report came back"))
            })
            .collect()
    }
}

/// One agent, start to finish.
fn work(runner: &Runner, workdir: &std::path::Path, task: &Dispatch, deadline: Duration) -> Report {
    let Some(role) = roster::find(&task.role) else {
        return Report::failed(
            &task.role,
            &format!("there is no role called {}", task.role),
        );
    };

    let began = Instant::now();
    let session = match SessionId::fresh() {
        Ok(s) => s,
        Err(e) => return Report::failed(role.name, &format!("no session id: {e}")),
    };

    // A fresh process per task rather than a pooled one. A worker exists for one job and has
    // no reason to remember the last, and a pool of them would keep twenty processes alive for
    // a run that finished an hour ago.
    let outcome = run_with_deadline(runner, workdir, role, &session, &task.instruction, deadline);

    match outcome {
        Ok(said) if said.trim().is_empty() => {
            Report::failed(role.name, "answered with nothing").after(began.elapsed())
        }
        Ok(said) => Report::done(role.name, &said).after(began.elapsed()),
        Err(why) => Report::failed(role.name, &why).after(began.elapsed()),
    }
}

/// Runs one agent, giving up if it takes too long.
fn run_with_deadline(
    runner: &Runner,
    workdir: &std::path::Path,
    role: &Role,
    session: &SessionId,
    instruction: &str,
    deadline: Duration,
) -> std::result::Result<String, String> {
    let mut open = runner
        .open_session(session, workdir, &roster::brief_for(role), false)
        .map_err(|e| format!("could not start: {e}"))?;

    let began = Instant::now();
    let mut said = String::new();

    let answer = open.ask(
        instruction,
        &mut |chunk| {
            said.push_str(chunk);
            crate::claude::Flow::Continue
        },
        &mut || {
            if began.elapsed() > deadline {
                crate::claude::Flow::Stop
            } else {
                crate::claude::Flow::Continue
            }
        },
    );

    match answer {
        Ok(a) if a.interrupted => Err(format!(
            "gave up after {:.0}s with {} characters written",
            deadline.as_secs_f32(),
            said.len()
        )),
        Ok(a) => Ok(a.text),
        Err(e) => Err(e.to_string()),
    }
}

/// Turns a set of reports into something a person can read.
pub fn summarise(reports: &[Report]) -> Summary {
    Summary::of(reports)
}

/// Checks a plan before any of it runs.
///
/// Cheaper to refuse a task naming a role that does not exist than to start it and find out.
/// With twenty tasks, one typo otherwise means one silent gap in a wall of output.
pub fn check(tasks: &[Dispatch]) -> Result<()> {
    let unknown: Vec<&str> = tasks
        .iter()
        .filter(|t| roster::find(&t.role).is_none())
        .map(|t| t.role.as_str())
        .collect();

    if unknown.is_empty() {
        return Ok(());
    }
    Err(crate::Error::Refused(format!(
        "no such role: {}. The roster is: {}",
        unknown.join(", "),
        roles()
            .iter()
            .map(|r| r.name)
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_plan_deploys_nobody() {
        let army = Army::new(Runner::at("/nonexistent"), "/tmp");
        assert!(army.deploy(&[]).is_empty());
    }

    /// One typo in a plan of twenty is one silent gap in a wall of output, so it is caught
    /// before anything starts.
    #[test]
    fn a_plan_naming_a_role_that_does_not_exist_is_refused() {
        let tasks = vec![Dispatch::new("nobody-by-that-name", "do a thing")];
        let err = check(&tasks).unwrap_err().to_string();
        assert!(err.contains("no such role"), "{err}");
        assert!(err.contains("The roster is"), "and says what is available");
    }

    #[test]
    fn a_plan_of_real_roles_passes() {
        let first = roles()[0].name;
        assert!(check(&[Dispatch::new(first, "do a thing")]).is_ok());
    }

    /// Always one report per task, even when everything fails. A caller counting answers
    /// should not have to work out which one is missing.
    #[test]
    fn every_task_comes_back_with_something() {
        let army = Army::new(Runner::at("/nonexistent/definitely-not-claude"), "/tmp");
        let first = roles()[0].name;
        let tasks: Vec<Dispatch> = (0..5)
            .map(|i| Dispatch::new(first, format!("job {i}")))
            .collect();

        let reports = army.deploy(&tasks);
        assert_eq!(reports.len(), 5);
        assert!(reports.iter().all(|r| r.failed.is_some()));
    }

    /// A run that reads differently every time cannot be compared with the last one.
    #[test]
    fn reports_come_back_in_the_order_they_were_asked_for() {
        let army = Army::new(Runner::at("/nonexistent"), "/tmp").at_once(4);
        let names: Vec<&str> = roles().iter().take(3).map(|r| r.name).collect();
        let tasks: Vec<Dispatch> = names.iter().map(|n| Dispatch::new(*n, "x")).collect();

        let reports = army.deploy(&tasks);
        let back: Vec<&str> = reports.iter().map(|r| r.role.as_str()).collect();
        assert_eq!(back, names);
    }

    /// However many tasks there are, only so many run at once. Twenty model calls arriving
    /// together is how nineteen of them meet a rate limit.
    #[test]
    fn the_number_running_at_once_is_bounded() {
        let army = Army::new(Runner::at("/nonexistent"), "/tmp").at_once(0);
        assert_eq!(army.at_once, 1, "zero would be nobody working at all");
    }
}
