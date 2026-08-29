//! One join across everything already known about the army, for somebody at a terminal.
//!
//! Three places hold the truth and each is authoritative for a different thing. `personnel` is
//! what an agent is and what it holds, written by the agent. `run/agents` is whether a process
//! exists, written only by the supervisor, which is the entire reason it is believed. The
//! journal is what happened, append only. Health is derived from the files themselves.
//!
//! Nothing here stores anything. It reads those four and puts them beside each other, because
//! the thing an operator actually needs is the row where they disagree: a process that is up,
//! a session that resumed, and a memory folder that is gone.

use std::path::Path;

use crate::Result;
use crate::army::personnel::Personnel;
use crate::army::runtime::{Lifecycle, Roll, Runtime};
use crate::army::{event, health, org};

/// Everything known about one agent, from every source that knows something.
#[derive(Debug, Clone)]
pub struct Standing {
    pub agent: &'static org::Agent,
    /// The agents this one may hand work to. Straight from the compiled table.
    pub reports: Vec<&'static org::Agent>,
    /// The task it is holding, when personnel says it holds one.
    pub holding: Option<String>,
    /// Which model its folder asks for.
    pub model: Option<String>,
    /// What the supervisor last wrote about its process. `None` means nobody has said, which is
    /// a different thing from stopped.
    pub runtime: Option<Runtime>,
    /// Whether it has a folder at all.
    pub enlisted: bool,
    pub health: health::Health,
}

impl Standing {
    /// The one line answer to "is anything wrong with this agent".
    ///
    /// A process being up is not the same as an agent being able to work. This is the place
    /// those two are allowed to disagree out loud.
    pub fn worry(&self) -> Option<String> {
        if !self.enlisted {
            return Some("no folder, so nothing can start it".into());
        }
        if self.health.memory.is_a_problem() {
            return Some(self.health.memory.why());
        }
        match &self.runtime.as_ref().map(|r| &r.lifecycle) {
            Some(Lifecycle::Degraded { why }) => Some(format!("degraded. {why}")),
            _ => None,
        }
    }
}

/// The whole army, in the order of the chart rather than alphabetically.
///
/// Chart order because the question is almost always about a department, and a list sorted by
/// name puts Miles between Mason and Nora where nobody is looking for him.
pub fn everyone(home: &Path) -> Result<Vec<Standing>> {
    let people = Personnel::open(home)?;
    // A home where no supervisor has ever run has no roll. That is "nobody has said",
    // not "nothing is running", and the two must not arrive as the same thing.
    let roll = Roll::open(home).ok();

    let mut out = Vec::new();
    for agent in org::everyone() {
        if agent.rank == org::Rank::Human {
            continue;
        }
        let folder = people.folder(agent.name);
        let enlisted = people.get(agent.name).is_some();

        out.push(Standing {
            agent,
            reports: org::reports_of(agent.name),
            holding: people
                .state(agent.name)
                .and_then(|s| s.holding.as_ref())
                .map(ToString::to_string),
            model: people.config(agent.name).map(|c| c.model.id().to_string()),
            // Matched by name, which the supervisor writes alongside the id for exactly this.
            runtime: roll
                .as_ref()
                .and_then(|r| r.all().find(|x| x.name == agent.name))
                .cloned(),
            enlisted,
            health: health::of(&folder),
        });
    }
    Ok(out)
}

/// One agent, or a refusal naming what it could have been.
pub fn one(home: &Path, name: &str) -> Result<Standing> {
    let wanted = org::require(name)?;
    everyone(home)?
        .into_iter()
        .find(|s| s.agent.name == wanted.name)
        .ok_or_else(|| crate::Error::Refused(format!("{name} has no standing to report")))
}

/// Recent things that actually happened, newest last, bounded.
///
/// Folded from the journal rather than from anywhere else. The journal is append only and is
/// already the durable history, so an activity store would be a second copy that can disagree
/// with it.
pub fn activity(home: &Path, about: Option<&str>, most: usize) -> Result<Vec<event::Record>> {
    let path = home.join("run").join("events.jsonl");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let mut records = event::read(&path)?;

    if let Some(who) = about {
        let who = org::require(who)?;
        records.retain(|r| r.actor == who.name || mentions(r, who.name));
    }
    let from = records.len().saturating_sub(most);
    Ok(records.split_off(from))
}

/// Whether a record is about this agent even though somebody else performed it.
///
/// A handoff to Miles is Olivia's act and Miles's business. Filtering on the actor alone would
/// show an agent everything it did and nothing that was done to it, which is the half that
/// explains why it is holding what it is holding.
///
/// The name has to be matched as a word rather than as a quoted JSON value. Both shapes are
/// real in the journal: an intervention carries `"to":"miles"`, and an enlistment carries the
/// name in a prose sentence. Looking only for the quoted form silently returned nothing for an
/// agent whose whole history was the prose kind.
fn mentions(record: &event::Record, name: &str) -> bool {
    serde_json::to_string(&record.event)
        .map(|json| names_agent(&json, name))
        .unwrap_or(false)
}

/// `name` appearing as a whole word, case insensitively.
///
/// Whole word so `nora` does not match inside a longer word, and case insensitive because the
/// same sentence carries both `miles` and `Miles`. If an event's text names an agent, that
/// event is that agent's business, whichever form it used.
fn names_agent(haystack: &str, name: &str) -> bool {
    let hay = haystack.to_lowercase();
    let needle = name.to_lowercase();
    let boundary = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());

    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        if boundary(hay[..start].chars().next_back()) && boundary(hay[end..].chars().next()) {
            return true;
        }
        from = end;
    }
    false
}

/// One word for a column, from a lifecycle that carries a whole sentence.
///
/// The reason belongs on the warning line, not in the column. Debug printing the variant put
/// the same sentence twice on every row and pushed the useful columns off the screen.
pub fn lifecycle_word(lifecycle: &Lifecycle) -> &'static str {
    match lifecycle {
        Lifecycle::Never => "never started",
        Lifecycle::Running { .. } => "running",
        Lifecycle::Exited { .. } => "exited",
        Lifecycle::Degraded { .. } => "degraded",
        Lifecycle::Stopped { .. } => "stopped",
        Lifecycle::Asleep { .. } => "asleep",
    }
}

/// One journal record as a line somebody can read.
///
/// Every variant is named rather than falling through to a debug dump, because a line that
/// prints its own struct is a line nobody reads twice. Where a variant carries a reason, the
/// reason is what is shown: "crashed" is a category and "crashed, exit 137" is a fact.
pub fn line_of(record: &event::Record) -> String {
    use event::Event as E;
    let what = match &record.event {
        E::Delegated { to, goal, task, .. } => format!("handed {task} to {to}. {goal}"),
        E::Moved { task, from, to } => format!("moved {task} from {from} to {to}"),
        E::Submitted { task, .. } => format!("submitted {task}"),
        E::Reviewed { task, .. } => format!("reviewed {task}"),
        E::Refused { what, why } => format!("refused {what}. {why}"),
        E::EmergencyDeclared { task, why } => format!("declared an emergency on {task}. {why}"),
        E::Decided { what, .. } => what.clone(),
        E::Intervened { what } => intervention(what),
        E::AgentStarted { name, .. } => format!("started {name}"),
        E::AgentCrashed {
            name,
            code,
            attempt,
            ..
        } => match code {
            Some(code) => format!("{name} crashed, exit {code}, attempt {attempt}"),
            None => format!("{name} crashed on signal, attempt {attempt}"),
        },
        E::AgentStartFailed { name, why, .. } => format!("{name} would not start. {why}"),
        E::AgentStopped { name, why, .. } => format!("stopped {name}. {why}"),
        E::AgentSlept { name, .. } => format!("{name} went to sleep"),
        E::AgentGaveUp { name, why, .. } => format!("gave up on {name}. {why}"),
        E::ContinuityChanged { name, .. } => format!("{name} came back with different continuity"),
        E::AgentWoken { name, .. } => format!("woke {name}"),
        E::Granted { .. } => "granted something".to_string(),
        E::Notified { who, about } => format!("told {who} about record {about}"),
    };
    format!(
        "{:>5}  {:<10} {}",
        record.seq,
        record.actor,
        one_line(&what)
    )
}

/// What JJ did, in words rather than as a struct.
fn intervention(what: &event::Intervention) -> String {
    use event::Intervention as I;
    match what {
        I::Message { to, what } => format!("JJ messaged {to}. {what}"),
        I::Objective { what } => format!("JJ set an objective. {what}"),
        I::Answered { question, .. } => format!("JJ answered question {question}"),
        I::Stopped { task, why } => format!("JJ stopped {task}. {why}"),
        I::Replaced { .. } => "JJ replaced a task".to_string(),
        I::Override { agent, instruction } => format!("JJ overrode {agent}. {instruction}"),
    }
}

/// Collapses a multi line reason so one record is one row.
fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(96) {
        Some((at, _)) => format!("{}...", &flat[..at]),
        None => flat,
    }
}

#[cfg(test)]
mod tests;
