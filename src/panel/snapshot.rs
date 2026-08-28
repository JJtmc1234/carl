//! Everything, at one moment, read from the three places that actually know.
//!
//! `org` for who exists, the personnel folders for what each agent holds, and the journal for
//! what happened. Nothing is cached between calls and nothing is written. Building a snapshot
//! twice in a row gives the same answer for the same inputs, which is the property that lets
//! the panel throw one away and take another whenever it is unsure.
//!
//! The sequence in the snapshot is the sequence of the last record it read. That is what makes
//! the snapshot and the stream join up exactly: subscribe from it and the next frame is the
//! next record, with nothing repeated and nothing skipped.

use super::facts::Facts;
use super::tasks;
use super::view::{AgentView, CarlView, LastEvent, Maybe, PanelSnapshot, Pending, ProcessState};
use crate::army::event::{self, Event, Record};
use crate::army::org;
use crate::army::personnel::Personnel;
use crate::{Error, Result};

/// How many recent handovers the Carl tab shows.
const RECENT: usize = 10;

/// Reads the whole world.
///
/// `home` is Carl's directory, the same one every other surface uses. An army that has never
/// been founded gives a snapshot with every agent present and unenlisted, rather than an error,
/// because the table is the organisation and the folders are only what has been written down
/// about it so far.
pub fn build(home: &std::path::Path) -> Result<PanelSnapshot> {
    let people = Personnel::open(home)?;
    let records = event::read(people.journal_path())?;
    build_from(&people, &records, &Facts::army_only().with_runtime(home))
}

/// The same, from state already in hand.
///
/// Split out so a test can build a snapshot without a directory, and so the server can reuse
/// records it has already read rather than reading the file twice on every connection.
pub fn build_from(people: &Personnel, records: &[Record], facts: &Facts) -> Result<PanelSnapshot> {
    let seq = records.last().map_or(0, |r| r.seq);
    let tasks = tasks::fold(records);

    let mut agents = Vec::new();
    for agent in org::everyone() {
        if agent.rank == org::Rank::Human {
            continue;
        }
        let folder = people.get(agent.name);
        let held = tasks::held_by(&tasks, agent.name);
        // Matched by name, which is what the supervisor writes alongside the id for exactly this.
        let runtime = facts.runtime.iter().find(|r| r.name == agent.name);

        agents.push(AgentView {
            name: agent.name.to_string(),
            display: agent.display.to_string(),
            rank: agent.rank,
            remit: agent.remit.to_string(),
            reports_to: agent.reports_to.map(str::to_string),
            department: folder.and_then(|f| f.profile.department.clone()),
            sub_department: folder.and_then(|f| f.profile.sub_department.clone()),
            enlisted: folder.is_some(),
            // The folder is the authority on what an agent is holding, because it is what
            // survives a restart. The fold is used for everything about that task except the
            // fact of holding it.
            holding: folder
                .and_then(|f| f.state.holding.as_ref())
                .map(|t| t.to_string()),
            task_status: held.map(|t| t.status.clone()).into(),
            blocked: held
                .map(|t| t.status == "changes_requested" && t.attempts >= crate::army::MAX_ATTEMPTS)
                .into(),
            last_event: last_by(records, agent.name).into(),
            model: folder.map(|f| f.config.model.id().to_string()).into(),
            // From the supervisor's records and from nowhere else. No record at all is Unknown
            // rather than not running, because "nobody has said" and "it is not running" are
            // different facts and only one of them means somebody should look.
            process: runtime.map(|r| ProcessState::of(&r.lifecycle)).into(),
            continuity: runtime.and_then(|r| r.continuity).into(),
        });
    }

    let recent_delegations = tasks
        .iter()
        .rev()
        .take(RECENT)
        .rev()
        .cloned()
        .collect::<Vec<_>>();

    Ok(PanelSnapshot {
        seq,
        at: event::now(),
        carl: CarlView {
            // Whether a conversation is open belongs to the turn machinery, which the server
            // holds and this function does not. The server fills it in.
            status: Maybe::Unknown,
            pending: pending_for_jj(records),
            objectives: objectives(records),
            recent_delegations,
        },
        agents,
        tasks,
        // From the providers, or empty when there are none. Empty is still never invented: a
        // project with no recorded milestones has none, and a machine nobody sampled has no
        // readings rather than zeroes.
        projects: facts.projects.clone(),
        diagnostics: facts.diagnostics.all().into_iter().cloned().collect(),
    })
}

/// The most recent record naming this agent as the one who acted.
fn last_by(records: &[Record], agent: &str) -> Option<LastEvent> {
    records
        .iter()
        .rev()
        .find(|r| r.actor == agent)
        .map(|r| LastEvent {
            seq: r.seq,
            at: r.at,
            kind: r.event.kind().to_string(),
            task: r.event.task().map(|t| t.to_string()),
        })
}

/// Questions put to JJ that JJ has not answered.
///
/// A question is `Decided` by an agent naming jj, and an answer is an `Intervened` `Answered`
/// carrying the sequence of the question. Tying the answer to a sequence rather than to matching
/// text is what makes this exact: two agents can ask the same question and the answers do not
/// cross.
fn pending_for_jj(records: &[Record]) -> Vec<Pending> {
    let answered: Vec<u64> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Intervened {
                what: crate::army::event::Intervention::Answered { question, .. },
            } => question.parse().ok(),
            _ => None,
        })
        .collect();

    records
        .iter()
        .filter(|r| r.actor != "jj")
        .filter_map(|r| match &r.event {
            Event::Decided { task, what } if asks_jj(what) => Some(Pending {
                seq: r.seq,
                at: r.at,
                asked_by: r.actor.clone(),
                question: what.clone(),
                task: task.as_ref().map(|t| t.to_string()),
            }),
            _ => None,
        })
        .filter(|p| !answered.contains(&p.seq))
        .collect()
}

/// Whether a decision was actually a question aimed at JJ.
///
/// Deliberately narrow. Carl is told to write "for JJ:" when something needs deciding above him,
/// and matching that marker is honest about what it knows. Guessing at question marks would
/// scoop up every rhetorical sentence in the record and fill the panel with things nobody asked.
fn asks_jj(what: &str) -> bool {
    let t = what.trim_start().to_lowercase();
    t.starts_with("for jj:") || t.starts_with("jj:")
}

/// What JJ has asked for that nobody has taken up yet.
///
/// Taken up means a delegation naming that objective's sequence exists. Matched on the sequence
/// rather than on the words, so two objectives worded alike stay two objectives, and one that
/// was handed down cannot be un-handed by somebody rewording it.
///
/// This used to list every objective ever set. The comment said it listed the open ones and the
/// code never filtered, so the panel showed a growing pile that nothing could ever remove and
/// JJ had no way to see which of them had actually moved.
fn objectives(records: &[Record]) -> Vec<String> {
    let taken_up: std::collections::BTreeSet<u64> = records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Delegated { objective, .. } => *objective,
            _ => None,
        })
        .collect();

    records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Intervened {
                what: crate::army::event::Intervention::Objective { what },
            } if !taken_up.contains(&r.seq) => Some(what.clone()),
            _ => None,
        })
        .collect()
}

/// One agent's folder and its recent record, for the inspect command.
pub fn inspect(people: &Personnel, records: &[Record], agent: &str) -> Result<AgentView> {
    let agent = org::require(agent)?;
    let snapshot = build_from(people, records, &Facts::army_only())?;
    snapshot
        .agents
        .into_iter()
        .find(|a| a.name == agent.name)
        .ok_or_else(|| Error::Refused(format!("{} is not an agent", agent.name)))
}
