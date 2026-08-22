//! Four real `claude` processes, and the bookkeeping that keeps them honest.
//!
//! One process per agent, held open for the whole run, because the conversation is the state.
//! Mason has to remember the task he wrote in order to review against it, and Nora has to
//! remember her first attempt in order to correct it. Restarting either between turns would
//! mean re-explaining the job every time and reviewing against a description rather than
//! against what was actually asked.
//!
//! Every process starts with its own brief and its own tool list, chosen by rank. That is the
//! second half of `org::check_may_implement`: the check refuses a chief who asks to implement,
//! and the empty tool list means a chief who never asks still cannot.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::staffing;
use super::words;
use super::{read_must, tools_for};
use crate::army::event::{Event, Journal};
use crate::army::org;
use crate::army::personnel::Personnel;
use crate::army::task::{Status, Task, TaskId, Verification};
use crate::claude::{Flow, Runner, Session};
use crate::{Error, Result, SessionId};

/// How long one agent may take on one turn.
///
/// Longer than the army's five minutes, because Nora's turn is a whole piece of work with
/// verification in it rather than one opinion.
pub const DEADLINE: Duration = Duration::from_secs(900);

/// How many times Nora may submit one task before it stops being hers.
///
/// The first go and two corrections, which is what JJ asked for. A third rejection means the
/// task itself is wrong, or the context she was given is, and both of those are Mason's boss's
/// problem rather than another round of the same.
pub const MAX_ATTEMPTS: u32 = 3;

pub struct Chain {
    program: PathBuf,
    workdir: PathBuf,
    deadline: Duration,
    journal: Journal,
    voices: Vec<(String, Session)>,
    /// Who holds an unfinished task, and which one. At most one entry per agent.
    open: Vec<(String, TaskId)>,
    /// Whether to print the chain walking past. On for a real run, off in tests.
    aloud: bool,
    /// The agent folders, if this chain has been given them.
    ///
    /// Optional because a chain has to be runnable against a temporary directory with no army
    /// founded in it, which is how every test here works. With folders it uses each agent's own
    /// deadline and rules, and who is holding what survives the process.
    people: Option<Personnel>,
}

/// The path one request actually took, for whoever has to see that it worked.
#[derive(Debug, Clone)]
pub struct Passage {
    pub request: String,
    pub for_adrian: String,
    pub for_mason: String,
    pub for_nora: String,
    /// The three tasks, in their final state.
    pub tasks: Vec<Task>,
    pub attempts: u32,
    pub accepted: bool,
    /// What Carl finally tells JJ.
    pub answer: String,
}

impl Passage {
    pub fn task(&self, owner: &str) -> Option<&Task> {
        self.tasks.iter().find(|t| t.owner == owner)
    }
}

impl Chain {
    pub fn new(
        program: impl Into<PathBuf>,
        workdir: impl Into<PathBuf>,
        journal: impl Into<PathBuf>,
    ) -> Result<Self> {
        Ok(Self {
            program: program.into(),
            workdir: workdir.into(),
            deadline: DEADLINE,
            journal: Journal::open(journal)?,
            voices: Vec::new(),
            open: Vec::new(),
            aloud: false,
            people: None,
        })
    }

    pub fn deadline(mut self, d: Duration) -> Self {
        self.deadline = d;
        self
    }

    /// Runs this chain against real agent folders.
    ///
    /// Anything the folders say is still outstanding is picked up here, so a chain restarted
    /// after a crash does not hand Nora a second task while the first is still open. That is the
    /// only reason the held task was ever worth writing to disk.
    pub fn staffed_by(mut self, people: Personnel) -> Self {
        self.open = staffing::outstanding(&people);
        self.people = Some(people);
        self
    }

    fn folder(&self, who: &str) -> Option<&crate::army::personnel::Folder> {
        self.people.as_ref().and_then(|p| p.get(who))
    }

    pub fn aloud(mut self, on: bool) -> Self {
        self.aloud = on;
        self
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn journal(&mut self) -> &mut Journal {
        &mut self.journal
    }

    /// One turn with one agent, in their own conversation.
    pub fn ask(&mut self, who: &str, prompt: &str) -> Result<String> {
        let agent = org::require(who)?;

        if !self.voices.iter().any(|(n, _)| n == who) {
            let session = SessionId::fresh()?;
            // The tool list is the rank. An empty one becomes no flag at all rather than an
            // empty flag, which some parsers read as allowing everything.
            let runner = Runner::at(&self.program).allowing(tools_for(agent.rank));
            // Built before the session opens, so a folder rule that tries to grant authority
            // stops the agent starting rather than being spoken and then regretted.
            let brief = staffing::brief(agent, self.folder(who))?;
            let open = runner.open_session(&session, &self.workdir, &brief, false)?;
            self.voices.push((who.to_string(), open));
        }

        let deadline = staffing::deadline(self.folder(who), self.deadline);
        let began = Instant::now();
        let voice = self
            .voices
            .iter_mut()
            .find(|(n, _)| n == who)
            .map(|(_, s)| s)
            .expect("just opened");

        let answer = voice.ask(prompt, &mut |_| Flow::Continue, &mut || {
            if began.elapsed() > deadline {
                Flow::Stop
            } else {
                Flow::Continue
            }
        })?;

        if answer.interrupted {
            return Err(Error::Claude(format!(
                "{who} was still going after {:.0}s",
                deadline.as_secs_f32()
            )));
        }
        if answer.text.trim().is_empty() {
            return Err(Error::Claude(format!("{who} answered with nothing")));
        }
        if self.aloud {
            eprintln!(
                "  {who:<7} {:.0}s, {} words",
                began.elapsed().as_secs_f32(),
                answer.text.split_whitespace().count()
            );
        }
        Ok(answer.text.trim().to_string())
    }

    /// Moves a task on and writes down that it moved.
    ///
    /// The record is built from the two statuses rather than described separately, so the task
    /// and the record cannot disagree about what happened.
    pub fn advance(&mut self, task: &mut Task, by: &str, next: Status) -> Result<()> {
        let from = task.status;
        if let Err(e) = task.advance(by, next) {
            self.refused(by, &format!("move task {} to {next}", task.id), &e)?;
            return Err(e);
        }
        self.journal
            .append(by, Event::moved(&task.id, from, next))?;
        if next.settled() {
            self.open.retain(|(_, id)| *id != task.id);
            self.put_down(&task.owner, &format!("{} {next}", task.id))?;
        }
        Ok(())
    }

    /// Creates a task from what an agent said, and records the handover.
    ///
    /// The conditions are asked for once more when they are missing rather than invented here.
    /// A task exists only with something checkable on it, and a generic condition written by
    /// the harness would satisfy the type while defeating the reason for it.
    pub fn hand_down(
        &mut self,
        from: &str,
        to: &str,
        said: &str,
        parent: Option<&Task>,
    ) -> Result<(Task, String, Vec<String>)> {
        self.check_free(to)?;

        let (mut goal, mut must) = read_must(said);
        if must.is_empty() {
            let again = self.ask(from, &words::conditions_again(said))?;
            let (g, m) = read_must(&again);
            goal = g;
            must = m;
        }

        let verification = Verification::of(must.clone()).inspect_err(|e| {
            let _ = self.refused(
                from,
                &format!("assign work to {to} with nothing checkable"),
                e,
            );
        })?;
        let goal = if goal.is_empty() {
            said.to_string()
        } else {
            goal
        };

        let made = match parent {
            Some(p) => Task::split_from(p, from, to, goal.clone(), verification),
            None => Task::assign(from, to, goal.clone(), verification),
        };
        let task = match made {
            Ok(t) => t,
            Err(e) => {
                self.refused(from, &format!("hand work to {to}"), &e)?;
                return Err(e);
            }
        };

        self.journal.append(
            from,
            Event::Delegated {
                workspace: task.workspace.clone(),
                task: task.id.clone(),
                to: to.to_string(),
                goal: task.goal.clone(),
                parent: task.parent.clone(),
                must: task.verification.must.clone(),
                project: task.project.clone(),
            },
        )?;
        self.now_holding(to, &task.id)?;
        Ok((task, goal, must))
    }

    pub fn submitted(&mut self, task: &TaskId, attempt: u32, words: usize) -> Result<()> {
        self.journal.append(
            "nora",
            Event::Submitted {
                task: task.clone(),
                attempt,
                words,
            },
        )?;
        Ok(())
    }

    pub fn reviewed(&mut self, by: &str, task: &TaskId, accepted: bool, why: &str) -> Result<()> {
        self.journal.append(
            by,
            Event::Reviewed {
                task: task.clone(),
                accepted,
                why: why.to_string(),
            },
        )?;
        Ok(())
    }

    pub fn decided(&mut self, by: &str, task: Option<&TaskId>, what: &str) -> Result<()> {
        self.journal.append(
            by,
            Event::Decided {
                task: task.cloned(),
                what: what.to_string(),
            },
        )?;
        Ok(())
    }

    /// Writes down something a rule stopped.
    ///
    /// The interesting half of the record. Without it nobody can tell a rule that is working
    /// from a rule nothing has ever hit.
    fn refused(&mut self, actor: &str, what: &str, why: &Error) -> Result<()> {
        self.journal.append(
            actor,
            Event::Refused {
                what: what.to_string(),
                why: why.to_string(),
            },
        )?;
        Ok(())
    }

    /// The unfinished task this agent is holding, if any.
    pub fn holding(&self, who: &str) -> Option<&TaskId> {
        self.open.iter().find(|(n, _)| n == who).map(|(_, id)| id)
    }

    /// Records that somebody has been handed a task and has not finished it.
    pub fn now_holding(&mut self, owner: &str, id: &TaskId) -> Result<()> {
        self.open.retain(|(n, _)| n != owner);
        self.open.push((owner.to_string(), id.clone()));

        let now = crate::army::event::now();
        if let Some(people) = self.people.as_mut() {
            people.update_state(owner, |s| s.take_up(id, now))?;
        }
        Ok(())
    }

    /// The other half of `now_holding`, so the folder does not keep saying somebody is busy.
    fn put_down(&mut self, owner: &str, why: &str) -> Result<()> {
        let now = crate::army::event::now();
        if let Some(people) = self.people.as_mut()
            && people.get(owner).is_some_and(|f| f.state.holding.is_some())
        {
            people.update_state(owner, |s| s.put_down(why, now))?;
        }
        Ok(())
    }

    /// Refuses giving somebody a second task while they still hold one.
    ///
    /// The task type counts attempts and checks who moves what, but nothing in it knows that
    /// two tasks exist at once, so this is the runtime's to enforce. A worker holding two jobs
    /// picks one and quietly drops the other, and the dropped one still reads as assigned.
    pub fn check_free(&mut self, who: &str) -> Result<()> {
        let Some(held) = self.holding(who).cloned() else {
            return Ok(());
        };
        let e = Error::Refused(format!(
            "{who} already holds task {held} and takes one task at a time. Finish that one \
             first."
        ));
        self.refused(who, &format!("hand a second task to {who}"), &e)?;
        Err(e)
    }
}

/// What happens after a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Accepted. The task is finished.
    Accept,
    /// Rejected with attempts left. It goes back to its owner.
    Correct,
    /// Rejected for the last time. It is abandoned and goes up a level.
    GiveUp,
}

/// The retry rule, on its own so it can be checked without a model.
///
/// One attempt and two corrections. The third rejection ends it, because a task rejected three
/// times is usually a task that was written wrong rather than one done wrong, and a fourth
/// round of the same is how a chain spends an afternoon going nowhere.
pub fn after_review(accepted: bool, attempts: u32) -> Next {
    if accepted {
        Next::Accept
    } else if attempts >= MAX_ATTEMPTS {
        Next::GiveUp
    } else {
        Next::Correct
    }
}
