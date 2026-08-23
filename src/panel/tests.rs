//! Tests for the panel backend.
//!
//! The socket ones talk over a real `UnixStream` between two threads rather than calling the
//! handler directly. A protocol tested by calling its own functions proves the functions agree
//! with themselves, which is not the question. The question is whether something on the other
//! end of a socket, holding only the schema, gets what it was promised.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use super::command::{self, PanelCommand};
use super::facts::Facts;
use super::snapshot;
use super::view::Maybe;
use super::wire::{Ask, Frame, Reply, Request, VERSION};
use super::{listen, serve, tasks};
use crate::army::event::{self, Event, Intervention, Journal, Record};
use crate::army::personnel::{Personnel, found};
use crate::army::task::{Status, Task, Verification};

fn army(home: &std::path::Path) -> Personnel {
    found(home, 1).unwrap()
}

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

/// Walks a real task from Mason to Nora and back, writing what the chain writes.
///
/// Built out of the same `Event` values `chain::Chain` appends, so a change that broke the panel
/// would have to break this too rather than only breaking a hand written fixture.
fn a_real_run(journal: &mut Journal) -> Task {
    let mut t = Task::assign(
        "mason",
        "nora",
        "cache the prototype lookup",
        verification(),
    )
    .unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: t.parent.clone(),
                must: t.verification.must.clone(),
                project: None,
                workspace: None,
            },
        )
        .unwrap();

    for (by, to) in [("nora", Status::InHand), ("nora", Status::Submitted)] {
        let from = t.status;
        t.advance(by, to).unwrap();
        journal.append(by, Event::moved(&t.id, from, to)).unwrap();
    }
    journal
        .append(
            "nora",
            Event::Submitted {
                task: t.id.clone(),
                attempt: t.attempts,
                words: 420,
            },
        )
        .unwrap();
    t
}

// ───────────────────────────── snapshot ─────────────────────────────

/// The snapshot must be the compiled table, not a copy of it that can fall behind.
#[test]
fn the_snapshot_shows_the_real_organisation() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let snap = snapshot::build_from(&people, &[], &Facts::army_only()).unwrap();

    let names: Vec<&str> = snap.agents.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        crate::army::org::everyone()
            .iter()
            .filter(|a| a.rank != crate::army::org::Rank::Human)
            .map(|a| a.name)
            .collect::<Vec<_>>(),
        "every agent in the table, in the order the table gives them"
    );
    assert!(
        !names.contains(&"jj"),
        "JJ is not an agent and has no folder"
    );

    let nora = snap.agents.iter().find(|a| a.name == "nora").unwrap();
    assert_eq!(nora.reports_to.as_deref(), Some("mason"));
    assert_eq!(nora.rank, crate::army::Rank::Worker);
    assert!(nora.enlisted, "founding wrote her folder");

    // Nothing measures a process yet, and a claim nobody checked would render a dead agent as
    // merely idle.
    assert!(nora.process.is_unknown(), "not measured, so not claimed");
}

/// The folder is what survives a restart, so the folder is what "holding" means.
#[test]
fn the_snapshot_reflects_what_the_folder_says_she_is_holding() {
    let dir = tempfile::tempdir().unwrap();
    let mut people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();
    let t = a_real_run(&mut journal);
    people
        .update_state("nora", |s| s.take_up(&t.id, 2))
        .unwrap();

    let records = event::read(people.journal_path()).unwrap();
    let snap = snapshot::build_from(&people, &records, &Facts::army_only()).unwrap();
    let nora = snap.agents.iter().find(|a| a.name == "nora").unwrap();

    assert_eq!(nora.holding.as_deref(), Some(t.id.as_str()));
    assert_eq!(nora.task_status, Maybe::known("submitted".to_string()));
    assert_eq!(snap.seq, records.last().unwrap().seq, "joins to the stream");
}

/// A task exists nowhere but the record, so the fold has to put every field back.
#[test]
fn a_task_is_rebuilt_from_the_record_with_nothing_lost() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();
    let t = a_real_run(&mut journal);

    let view = &tasks::fold(&event::read(people.journal_path()).unwrap())[0];
    assert_eq!(view.id, t.id.to_string());
    assert_eq!(view.goal, "cache the prototype lookup");
    assert_eq!(view.owner, "nora");
    assert_eq!(
        view.assigner, "mason",
        "who assigned it, and so who reviews it"
    );
    assert_eq!(view.status, "submitted");
    assert_eq!(view.attempts, 1);
    assert_eq!(view.must, vec!["cargo test passes"]);
    assert!(view.review.is_unknown(), "nobody has reviewed it yet");
}

/// The fold works in strings and `Status` is an enum. They have to mean the same thing.
#[test]
fn the_folds_idea_of_finished_matches_the_types() {
    for s in [Status::Accepted, Status::Abandoned] {
        assert!(tasks::settled(&s.to_string()), "{s} is finished");
        assert!(s.settled(), "and the type agrees");
    }
    for s in [
        Status::Assigned,
        Status::InHand,
        Status::Submitted,
        Status::ChangesRequested,
    ] {
        assert!(!tasks::settled(&s.to_string()), "{s} is not finished");
        assert!(!s.settled(), "and the type agrees");
    }
}

// ───────────────────────────── JJ interventions ─────────────────────────────

/// The whole reason interventions have their own variant.
#[test]
fn a_jj_intervention_is_never_recorded_as_an_ordinary_delegation() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();
    let before = event::read(people.journal_path()).unwrap().len();

    let done = command::record(
        &mut journal,
        Intervention::Message {
            to: "nora".into(),
            what: "stop what you are doing and check the belt".into(),
        },
    )
    .unwrap();

    let new: Vec<Record> = event::read(people.journal_path())
        .unwrap()
        .into_iter()
        .skip(before)
        .collect();

    let it = &new[0];
    assert_eq!(it.actor, "jj", "attributed to JJ and to nobody else");
    assert_eq!(it.event.kind(), "intervened");
    assert!(
        !new.iter().any(|r| r.event.kind() == "delegated"),
        "nothing here may look like Mason assigning her work"
    );
    assert_eq!(done.seq, it.seq);
}

/// Carl is accountable for the army and the lead thought they knew what their report was doing.
#[test]
fn carl_and_the_affected_lead_are_both_told() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();

    let done = command::record(
        &mut journal,
        Intervention::Override {
            agent: "nora".into(),
            instruction: "leave the cache alone for now".into(),
        },
    )
    .unwrap();

    assert_eq!(done.told, vec!["carl", "mason"], "the chief and her lead");

    let told: Vec<Record> = event::read(people.journal_path())
        .unwrap()
        .into_iter()
        .filter(|r| r.event.kind() == "notified")
        .collect();
    assert_eq!(told.len(), 2);
    for r in &told {
        match &r.event {
            Event::Notified { about, .. } => assert_eq!(
                *about, done.seq,
                "a notification points at what it notifies about rather than repeating it"
            ),
            other => panic!("wrong event: {other:?}"),
        }
    }
}

/// Carl's own lead is JJ, so telling him twice would be telling him once with extra noise.
#[test]
fn nobody_is_told_the_same_thing_twice() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();

    let done = command::record(
        &mut journal,
        Intervention::Message {
            to: "carl".into(),
            what: "handle this yourself".into(),
        },
    )
    .unwrap();
    assert_eq!(done.told, vec!["carl"]);
}

/// A stopped task must show as stopped, or the panel would render it as still running.
#[test]
fn a_stopped_task_shows_as_abandoned_while_the_record_still_says_jj_did_it() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();
    let t = a_real_run(&mut journal);

    command::record(
        &mut journal,
        Intervention::Stopped {
            task: t.id.clone(),
            why: "the approach is wrong".into(),
        },
    )
    .unwrap();

    let records = event::read(people.journal_path()).unwrap();
    assert_eq!(tasks::fold(&records)[0].status, "abandoned");

    let who = records
        .iter()
        .rev()
        .find(|r| r.event.kind() == "intervened")
        .unwrap();
    assert_eq!(who.actor, "jj", "and the record still says who decided");
}

// ───────────────────────────── command validation ─────────────────────────────

#[test]
fn a_command_naming_nobody_is_refused_before_anything_is_written() {
    for bad in [
        PanelCommand::JjMessage {
            agent: "hunter".into(),
            text: "hello".into(),
        },
        // JJ is in the table but is not an agent, so there is nobody to send to.
        PanelCommand::JjStop {
            agent: "jj".into(),
            why: "no".into(),
        },
        PanelCommand::Say { text: "   ".into() },
        PanelCommand::Objective { text: "".into() },
        PanelCommand::JjReplace {
            agent: "nora".into(),
            goal: "do the thing".into(),
            why: " ".into(),
        },
        PanelCommand::Answer {
            seq: 0,
            text: "yes".into(),
        },
    ] {
        assert!(bad.check().is_err(), "should have been refused: {bad:?}");
    }
}

/// The security property, stated as a test so it cannot be quietly removed.
///
/// There is no actor on a command. The socket is the authentication, so there is no field for a
/// caller to write "mason" into and be believed.
#[test]
fn a_command_cannot_claim_to_come_from_an_agent() {
    let raw = serde_json::to_value(PanelCommand::JjStop {
        agent: "nora".into(),
        why: "stop".into(),
    })
    .unwrap();
    let keys: Vec<&String> = raw.as_object().unwrap().keys().collect();
    assert!(
        keys.iter().any(|k| k.as_str() == "kind"),
        "tagged by kind: {keys:?}"
    );

    for forbidden in ["actor", "as", "by", "on_behalf_of", "rank"] {
        assert!(
            !keys.iter().any(|k| k.as_str() == forbidden),
            "a command must not carry {forbidden}: {keys:?}"
        );
    }

    // And an extra field is rejected rather than ignored, so a hopeful caller finds out.
    let mut hopeful = raw.clone();
    hopeful
        .as_object_mut()
        .unwrap()
        .insert("actor".into(), serde_json::json!("mason"));
    assert!(serde_json::from_value::<PanelCommand>(hopeful).is_err());
}

// ───────────────────────────── the socket, for real ─────────────────────────────

/// A connected panel, on the other end of a real socket.
struct Panel {
    out: UnixStream,
    lines: BufReader<UnixStream>,
    n: u32,
}

impl Panel {
    fn connect(home: &std::path::Path) -> Self {
        let at = listen::socket_path(home);
        let stream = UnixStream::connect(&at).unwrap();
        Self {
            out: stream.try_clone().unwrap(),
            lines: BufReader::new(stream),
            n: 0,
        }
    }

    fn send(&mut self, body: Ask) -> String {
        self.n += 1;
        let id = format!("r{}", self.n);
        let request = Request {
            v: VERSION,
            id: id.clone(),
            body,
        };
        writeln!(self.out, "{}", serde_json::to_string(&request).unwrap()).unwrap();
        self.out.flush().unwrap();
        id
    }

    fn raw(&mut self, line: &str) {
        writeln!(self.out, "{line}").unwrap();
        self.out.flush().unwrap();
    }

    fn next(&mut self) -> Frame {
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("unreadable frame {line:?}: {e}"))
    }
}

/// Starts a real backend on a real socket and hands back its home.
fn backend() -> (tempfile::TempDir, Personnel) {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let held = listen::hold(&listen::socket_path(dir.path())).unwrap();
    let home = dir.path().to_path_buf();
    std::thread::spawn(move || {
        let _ = serve::Server::new(&home).run(held);
    });
    (dir, people)
}

#[test]
fn a_panel_connects_and_gets_the_real_organisation_over_the_socket() {
    let (dir, _people) = backend();
    let mut panel = Panel::connect(dir.path());

    let id = panel.send(Ask::Ping);
    let frame = panel.next();
    assert_eq!(
        frame.id.as_deref(),
        Some(id.as_str()),
        "the reply is matched"
    );
    assert_eq!(frame.body, Reply::Pong);

    panel.send(Ask::Snapshot);
    match panel.next().body {
        Reply::Snapshot { snapshot } => {
            assert_eq!(
                snapshot.agents.len(),
                crate::army::org::everyone()
                    .iter()
                    .filter(|a| a.rank != crate::army::org::Rank::Human)
                    .count()
            );
            // No project has been created in this home, so there are none. Empty because nothing
            // was written rather than because nothing was asked.
            assert!(snapshot.projects.is_empty(), "none were created");
            // Diagnostics are real now, and the two kinds have to stay apart on the wire.
            assert!(
                !snapshot.diagnostics.is_empty(),
                "the providers are wired in"
            );
            assert!(
                snapshot
                    .diagnostics
                    .iter()
                    .any(|d| d.kind == crate::providers::Kind::Sampled && d.measured_at.is_some()),
                "a sampled reading carries when it was read"
            );
            assert!(
                snapshot
                    .diagnostics
                    .iter()
                    .any(|d| d.kind == crate::providers::Kind::EventDriven),
                "and army state does not pretend to have been measured at an instant"
            );
        }
        other => panic!("wrong reply: {other:?}"),
    }
}

/// The one that matters. An event written by the army, by another writer, while a panel is
/// already subscribed, with no refresh and nothing asked for.
#[test]
fn an_event_written_by_the_army_reaches_a_subscribed_panel_live() {
    let (dir, people) = backend();
    let mut panel = Panel::connect(dir.path());

    panel.send(Ask::Subscribe { since: 0 });
    // Founding wrote enlistment records, so drain to the live marker first.
    let caught_up = loop {
        match panel.next().body {
            Reply::Live { seq } => break seq,
            Reply::Event { .. } => continue,
            other => panic!("wrong reply: {other:?}"),
        }
    };

    // A completely separate Journal handle, the way the chain writes from its own process.
    let mut journal = Journal::open(people.journal_path()).unwrap();
    let t = a_real_run(&mut journal);

    let mut seen = Vec::new();
    while seen.len() < 4 {
        match panel.next().body {
            Reply::Event { event } => {
                assert!(event.seq > caught_up, "and in order after the catch up");
                seen.push(event.kind.clone());
            }
            // The machine is sampled whether or not the army is busy, so these interleave. The
            // property worth checking is that one can never be mistaken for history.
            Reply::Telemetry { at, diagnostics } => {
                assert!(at > 0, "a sample knows when it was taken");
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d.kind == crate::providers::Kind::Sampled),
                    "only sampled telemetry is pushed; army state travels as events"
                );
            }
            other => panic!("wrong reply: {other:?}"),
        }
    }
    assert_eq!(seen, vec!["delegated", "moved", "moved", "submitted"]);

    // Machine readable, not prose. The panel can read the task straight out of the record.
    panel.send(Ask::Ping);
    assert_eq!(
        tasks::fold(&event::read(people.journal_path()).unwrap())[0].id,
        t.id.to_string()
    );
}

#[test]
fn a_panel_that_reconnects_gets_what_it_missed_and_no_more() {
    let (dir, people) = backend();
    let mut journal = Journal::open(people.journal_path()).unwrap();

    let seen_up_to = {
        let mut panel = Panel::connect(dir.path());
        panel.send(Ask::Subscribe { since: 0 });
        loop {
            if let Reply::Live { seq } = panel.next().body {
                break seq;
            }
        }
    };

    // Away. Two things happen while nothing is connected.
    a_real_run(&mut journal);
    let after = event::read(people.journal_path())
        .unwrap()
        .last()
        .unwrap()
        .seq;

    let mut panel = Panel::connect(dir.path());
    panel.send(Ask::Subscribe { since: seen_up_to });
    let mut replayed = Vec::new();
    let live_at = loop {
        match panel.next().body {
            Reply::Event { event } => replayed.push(event.seq),
            Reply::Live { seq } => break seq,
            other => panic!("wrong reply: {other:?}"),
        }
    };

    assert_eq!(
        replayed,
        (seen_up_to + 1..=after).collect::<Vec<_>>(),
        "exactly what it missed, in order, with nothing repeated"
    );
    assert_eq!(live_at, after);
}

/// A hole served as a continuous stream is worse than an error, because it looks fine.
#[test]
fn a_sequence_this_record_cannot_honour_is_answered_rather_than_faked() {
    let (dir, _people) = backend();
    let mut panel = Panel::connect(dir.path());

    panel.send(Ask::Subscribe { since: 9999 });
    match panel.next().body {
        Reply::Gap { asked_for, why, .. } => {
            assert_eq!(asked_for, 9999);
            assert!(why.contains("fresh snapshot"), "and says what to do: {why}");
        }
        other => panic!("wrong reply: {other:?}"),
    }
}

#[test]
fn rubbish_on_the_socket_is_answered_rather_than_dropped() {
    let (dir, _people) = backend();
    let mut panel = Panel::connect(dir.path());

    panel.raw("{ this is not json");
    match panel.next().body {
        Reply::Refused { why } => assert!(why.contains("unreadable"), "{why}"),
        other => panic!("wrong reply: {other:?}"),
    }

    // A frame from a future protocol is refused with both versions named, rather than parsed
    // hopefully and misunderstood.
    panel.raw(r#"{"v":99,"id":"x","ask":"ping"}"#);
    match panel.next().body {
        Reply::Refused { why } => assert!(why.contains("99") && why.contains('1'), "{why}"),
        other => panic!("wrong reply: {other:?}"),
    }

    // And the connection is still usable, because one bad line is not a reason to hang up.
    panel.send(Ask::Ping);
    assert_eq!(panel.next().body, Reply::Pong);
}

/// Stopping a task nobody is holding must not put a stop event against an invented id.
#[test]
fn stopping_somebody_who_is_doing_nothing_is_refused() {
    let (dir, people) = backend();
    let before = event::read(people.journal_path()).unwrap().len();

    let mut panel = Panel::connect(dir.path());
    panel.send(Ask::Command {
        command: PanelCommand::JjStop {
            agent: "nora".into(),
            why: "stop".into(),
        },
    });
    match panel.next().body {
        Reply::Refused { why } => assert!(why.contains("not holding a task"), "{why}"),
        other => panic!("wrong reply: {other:?}"),
    }

    assert_eq!(
        event::read(people.journal_path()).unwrap().len(),
        before,
        "and nothing was written down"
    );
}

/// A command over the socket has to reach the same recording path the unit tests check.
#[test]
fn a_command_over_the_socket_lands_in_the_real_journal() {
    let (dir, people) = backend();
    let mut panel = Panel::connect(dir.path());

    panel.send(Ask::Command {
        command: PanelCommand::JjInstruct {
            agent: "nora".into(),
            instruction: "check the belt before the inserter".into(),
        },
    });
    let seq = match panel.next().body {
        Reply::Done { seq, what } => {
            assert!(what.contains("carl") && what.contains("mason"), "{what}");
            seq.unwrap()
        }
        other => panic!("wrong reply: {other:?}"),
    };

    let records = event::read(people.journal_path()).unwrap();
    let it = records.iter().find(|r| r.seq == seq).unwrap();
    assert_eq!(it.actor, "jj");
    assert_eq!(it.event.kind(), "intervened");
}

/// Found by running it. Every JJ intervention filed itself under JJ, so the Agents tab would
/// have shown nothing at all against the agent it actually happened to.
#[test]
fn a_frame_is_filed_under_who_it_is_about_not_who_acted() {
    use super::wire::{Entity, PanelEvent};

    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();

    command::record(
        &mut journal,
        Intervention::Override {
            agent: "nora".into(),
            instruction: "leave the cache alone".into(),
        },
    )
    .unwrap();

    let filed: Vec<(String, String)> = event::read(people.journal_path())
        .unwrap()
        .into_iter()
        // Founding wrote a line per enlistment, which is genuinely about whoever did the
        // enlisting. Only the three from the intervention are interesting here.
        .filter(|r| matches!(r.event.kind(), "intervened" | "notified"))
        .map(PanelEvent::of)
        .map(|e| {
            let who = match e.entity {
                Entity::Agent { name } => name,
                Entity::Task { id, .. } => id,
            };
            (e.kind, who)
        })
        .collect();

    assert_eq!(
        filed,
        vec![
            ("intervened".into(), "nora".into()),
            ("notified".into(), "carl".into()),
            ("notified".into(), "mason".into()),
        ],
        "the actor is jj throughout, and none of these belong on jj's row"
    );
}
