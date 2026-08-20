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
use super::view::{AgentView, CarlView, LastEvent, Maybe, PanelSnapshot, Pending};
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
    let (records, people) = read_settled(home)?;
    build_from(&people, &records, &Facts::army_only())
}

/// How many times to re read before accepting a torn pair.
///
/// Small, because the window is two adjacent writes and closes in microseconds. A snapshot that
/// is briefly late is fine. A snapshot that never returns is not.
const SETTLE_TRIES: usize = 8;

/// Reads the journal and the agent folders as one consistent pair.
///
/// The two reads cannot be atomic, and reordering them is not enough, which is worth writing
/// down because it is the obvious fix and it does not work. The chain appends the `delegated`
/// record and then writes the agent's state file. Whichever order a reader uses, if both of its
/// reads land inside that gap it sees a sequence that has passed the delegation while the agent
/// holds nothing. Reordering only narrows the window from "either read lands in the gap" to
/// "both do". Measured: still four runs in twelve. See bug 23.
///
/// So it is read again instead of reasoned about. The gap closes as fast as one file write, so
/// a re read almost always settles at once, and after `SETTLE_TRIES` the pair is returned
/// anyway, because a snapshot that is briefly wrong beats one that never arrives.
///
/// Journal first inside each attempt, so the tear that does survive is the recoverable one: an
/// agent holding a task the fold has not seen yet, corrected by that record arriving on the
/// stream above the snapshot's sequence.
pub fn read_settled(home: &std::path::Path) -> Result<(Vec<Record>, Personnel)> {
    let mut last = None;
    for attempt in 0..SETTLE_TRIES {
        let records = event::read(crate::army::personnel::journal_path_in(home))?;
        let people = Personnel::open(home)?;

        if !behind(&records, &people) {
            return Ok((records, people));
        }
        last = Some((records, people));
        if attempt + 1 < SETTLE_TRIES {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    // Unreachable in practice, and returned rather than erroring because a caller asking for a
    // snapshot needs one.
    last.ok_or_else(|| crate::Error::Refused("could not read a settled snapshot".into()))
}

/// Whether the folders are older than the journal, which is the tear that never heals.
///
/// True when the fold says an agent owns an unfinished task and that agent's folder shows them
/// holding nothing.
fn behind(records: &[Record], people: &Personnel) -> bool {
    let tasks = tasks::fold(records);
    tasks.iter().any(|t| {
        !tasks::settled(&t.status)
            && people.get(&t.owner).is_some_and(|f| {
                f.state.holding.as_ref().map(|h| h.as_str()) != Some(t.id.as_str())
            })
    })
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
            // Nothing measures this yet. Saying `Known(false)` would be a claim nobody checked,
            // and a dead agent would render as merely idle.
            process: Maybe::Unknown,
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

/// What JJ has asked for that nothing has closed yet.
fn objectives(records: &[Record]) -> Vec<String> {
    records
        .iter()
        .filter_map(|r| match &r.event {
            Event::Intervened {
                what: crate::army::event::Intervention::Objective { what },
            } => Some(what.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::army::event::{Event, Journal};
    use crate::army::personnel::found;
    use crate::army::task::TaskId;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Writes a delegation the way the chain does: the journal record first, then the state
    /// file. That order is not negotiable here, it is what the chain actually does, and it is
    /// what makes the read order matter.
    fn delegate(home: &std::path::Path, task: &TaskId) {
        let mut journal = Journal::open(crate::army::personnel::journal_path_in(home)).unwrap();
        journal
            .append(
                "mason",
                Event::Delegated {
                    task: task.clone(),
                    to: "nora".into(),
                    goal: "do the thing".into(),
                    parent: None,
                    must: vec!["it works".into()],
                    project: None,
                },
            )
            .unwrap();

        let now = crate::army::event::now();
        let mut people = Personnel::open(home).unwrap();
        people
            .update_state("nora", |s| s.take_up(task, now))
            .unwrap();
    }

    /// The bug. `everything` and `build` opened `Personnel` first, which eagerly reads every
    /// state file, and read the journal second. The chain writes the other way round, journal
    /// then state file, so a snapshot taken in between carried a sequence that already included
    /// the delegation while the agent showed as holding nothing.
    ///
    /// That combination is the one that never heals. The panel subscribes from the snapshot's
    /// sequence, so the `delegated` record is behind it and is never replayed, and the row
    /// stays wrong until something unrelated forces a resync.
    ///
    /// The reverse tear is fine and is what this now produces: an agent holding a task the fold
    /// has not seen yet, corrected by the record arriving on the stream above the snapshot.
    ///
    /// Racy on purpose, because the bug only exists in the window between two writes. It fails
    /// only when it actually catches the bad combination, so a quiet run is not a false pass,
    /// it is a run that did not hit the window.
    #[test]
    fn a_snapshot_never_shows_a_delegation_the_agent_does_not_yet_hold() {
        let dir = tempfile::tempdir().unwrap();
        found(dir.path(), crate::army::event::now()).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let home = dir.path().to_path_buf();
            let stop = stop.clone();
            std::thread::spawn(move || {
                for i in 0..300 {
                    if stop.load(Ordering::Relaxed) {
                        break;
                    }
                    let task = TaskId::quoted(format!("task{i:04}"));
                    delegate(&home, &task);
                }
            })
        };

        for _ in 0..400 {
            let snap = build(dir.path()).unwrap();
            let nora = snap
                .agents
                .iter()
                .find(|a| a.name == "nora")
                .expect("nora is in the army");

            // The fold has seen a task owned by nora, so the journal read included a
            // delegation to her. The folder must not be older than that read.
            let folded = snap.tasks.iter().any(|t| t.owner == "nora");
            if folded {
                assert!(
                    nora.holding.is_some(),
                    "the snapshot shows a delegation to nora at seq {} while her folder holds \\
                     nothing, which is the tear the panel can never recover from",
                    snap.seq
                );
            }
        }

        stop.store(true, Ordering::Relaxed);
        let _ = writer.join();
    }
}
