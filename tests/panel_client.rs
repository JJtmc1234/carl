//! The typed client, against a real backend on a real socket.
//!
//! An integration test rather than a unit one, because the questions worth asking are about two
//! processes' worth of behaviour: what happens when the server goes away mid stream, what a
//! reconnect resumes from, and whether a client ever reports a screen as current when it is not.
//! None of that can be asked of a function.
//!
//! The backend here is started and stopped inside the test the way it is in real use, so a
//! restart is a genuine restart and not a mocked one.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carl::army::event::{Event, Intervention, Journal};
use carl::army::personnel::{Personnel, found};
use carl::army::task::{Status, Task, TaskId, Verification};
use carl::panel::PanelCommand;
use carl::panel::client::{Incoming, PanelClient};
use carl::panel::listen;
use carl::panel::live::{Health, LivePanel, Update};

/// The real `carl panel` binary, as a child process.
///
/// A thread inside the test process was tried first and was wrong in a way worth writing down:
/// unlinking a socket does not break connections that are already open, so a subscribed client
/// carried on being served by the old thread and never noticed anything. Nothing about reconnect
/// was being tested at all.
///
/// A real child process fixes that, because killing it closes every connection it holds. It also
/// means these tests exercise the binary JJ actually runs, including its signal handling.
struct Backend {
    home: PathBuf,
    child: Option<std::process::Child>,
}

impl Backend {
    fn start(home: &Path) -> Self {
        let mut me = Self {
            home: home.to_path_buf(),
            child: None,
        };
        me.up();
        me
    }

    /// Starts the real binary, and returns only once its socket is answering.
    ///
    /// Waiting for a real connection rather than sleeping a guessed interval, so nothing here
    /// races the process it just spawned.
    fn up(&mut self) {
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_carl"))
            .arg("--home")
            .arg(&self.home)
            .arg("panel")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("starting carl panel");
        self.child = Some(child);

        for _ in 0..400 {
            if PanelClient::connect(&self.socket()).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend never came up");
    }

    /// Stops it the way systemd would, and waits until it is really gone.
    fn down(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        for _ in 0..400 {
            if PanelClient::connect(&self.socket()).is_err() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend never went away");
    }

    fn socket(&self) -> PathBuf {
        listen::socket_path(&self.home)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

/// Writes what the chain writes, from a handle of its own.
fn a_real_delegation(journal: &mut Journal) -> TaskId {
    let mut t = Task::assign("mason", "nora", "cache the lookup", verification()).unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: t.parent.clone(),
                must: t.verification.must.clone(),
            },
        )
        .unwrap();
    let from = t.status;
    t.advance("nora", Status::InHand).unwrap();
    journal
        .append("nora", Event::moved(&t.id, from, Status::InHand))
        .unwrap();
    t.id
}

fn army(home: &Path) -> Personnel {
    found(home, 1).unwrap()
}

#[test]
fn a_snapshot_decodes_into_the_real_types() {
    let dir = tempfile::tempdir().unwrap();
    army(dir.path());
    let backend = Backend::start(dir.path());

    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    client.ping().unwrap();
    let snapshot = client.snapshot().unwrap();

    let nora = snapshot.agents.iter().find(|a| a.name == "nora").unwrap();
    assert_eq!(nora.rank, carl::army::Rank::Worker);
    assert_eq!(nora.reports_to.as_deref(), Some("mason"));
    assert!(
        nora.process.is_unknown(),
        "nothing measures it, so nothing claims it"
    );
    assert!(snapshot.projects.is_empty());
}

#[test]
fn a_command_round_trips_and_lands_in_the_real_journal() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let backend = Backend::start(dir.path());

    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    let done = client
        .command(PanelCommand::JjInstruct {
            agent: "nora".into(),
            instruction: "check the belt first".into(),
        })
        .unwrap();

    let seq = done.seq.expect("an intervention is written down");
    let records = carl::army::event::read(people.journal_path()).unwrap();
    let it = records.iter().find(|r| r.seq == seq).unwrap();
    assert_eq!(it.actor, "jj");
    assert_eq!(it.event.kind(), "intervened");
}

#[test]
fn a_refused_command_is_an_error_rather_than_a_done() {
    let dir = tempfile::tempdir().unwrap();
    army(dir.path());
    let backend = Backend::start(dir.path());

    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    let e = client
        .command(PanelCommand::JjStop {
            agent: "nora".into(),
            why: "stop".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(e.contains("not holding a task"), "{e}");
}

#[test]
fn a_subscription_arrives_in_order_with_no_holes() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let backend = Backend::start(dir.path());

    let mut events = PanelClient::connect(&backend.socket())
        .unwrap()
        .subscribe(0)
        .unwrap();
    while !matches!(events.recv().unwrap(), Incoming::CaughtUp { .. }) {}
    let caught_up = events.last_seq();

    let mut journal = Journal::open(people.journal_path()).unwrap();
    a_real_delegation(&mut journal);

    let mut seen = Vec::new();
    for _ in 0..2 {
        match events.recv().unwrap() {
            Incoming::Event(e) => seen.push((e.seq, e.kind.clone())),
            other => panic!("wrong frame: {other:?}"),
        }
    }
    assert_eq!(
        seen,
        vec![
            (caught_up + 1, "delegated".to_string()),
            (caught_up + 2, "moved".to_string())
        ]
    );
    assert_eq!(events.last_seq(), caught_up + 2);
}

/// The whole point of the reconnect helper.
#[test]
fn the_live_panel_reconnects_after_the_backend_restarts_and_resumes_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let mut backend = Backend::start(dir.path());

    let (mut live, snapshot) = LivePanel::open(&backend.socket()).unwrap();
    let live_at = snapshot.seq;
    assert_eq!(live.health(), Health::Connected);

    // The backend goes away. Work carries on regardless, which is exactly the case a panel must
    // not miss: the army does not stop because nobody is watching.
    backend.down();

    let mut journal = Journal::open(people.journal_path()).unwrap();
    a_real_delegation(&mut journal);
    let happened_up_to = carl::army::event::read(people.journal_path())
        .unwrap()
        .last()
        .unwrap()
        .seq;

    backend.up();

    // Everything that happened while it was away, in order, starting from exactly where the
    // client got to. Nothing is skipped and nothing is repeated.
    let mut got = Vec::new();
    let mut healths = Vec::new();
    while got.last().map(|(s, _)| *s) != Some(happened_up_to) {
        match live.next_update() {
            Update::Event(e) => got.push((e.seq, e.kind.clone())),
            Update::Health(h) => healths.push(h),
            Update::Resynced(_) => panic!("it could have resumed, so it should not have resynced"),
        }
    }

    assert_eq!(
        got.iter().map(|(s, _)| *s).collect::<Vec<_>>(),
        (live_at + 1..=happened_up_to).collect::<Vec<_>>(),
        "resumed from exactly where it was"
    );
    assert!(
        healths.contains(&Health::Reconnecting) || healths.contains(&Health::Stale),
        "and said so while it was away: {healths:?}"
    );
    assert_eq!(live.health(), Health::Connected);
}

/// The other branch. When resuming is impossible, fresh truth is the only honest answer.
#[test]
fn a_gap_becomes_a_fresh_snapshot_rather_than_a_guess() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let backend = Backend::start(dir.path());

    let mut journal = Journal::open(people.journal_path()).unwrap();
    carl::panel::command::record(
        &mut journal,
        Intervention::Objective {
            what: "make it faster".into(),
        },
    )
    .unwrap();

    let (mut live, _) = LivePanel::open(&backend.socket()).unwrap();

    // A record that cannot answer where this client is. Standing in for the journal having been
    // replaced under a running panel, which is the real way this happens.
    std::fs::write(people.journal_path(), "").unwrap();

    let mut journal = Journal::open(people.journal_path()).unwrap();
    a_real_delegation(&mut journal);

    let fresh = loop {
        match live.next_update() {
            Update::Resynced(s) => break s,
            Update::Health(_) => continue,
            Update::Event(e) => panic!("nothing should have been resumable: {}", e.seq),
        }
    };
    assert_eq!(
        live.last_seq(),
        fresh.seq,
        "and the stream continues from it"
    );
    assert_eq!(live.health(), Health::Connected);
}

/// A hole must stop the client rather than be smoothed over.
#[test]
fn a_stream_that_skips_a_sequence_is_an_error_not_a_value() {
    let dir = tempfile::tempdir().unwrap();
    let people = army(dir.path());
    let backend = Backend::start(dir.path());

    let mut journal = Journal::open(people.journal_path()).unwrap();
    a_real_delegation(&mut journal);

    // A record with a sequence jumped forward, which no honest writer produces.
    let path = people.journal_path();
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str(
        r#"{"seq":900,"at":1,"actor":"mason","event":"decided","task":null,"what":"leapt"}"#,
    );
    text.push('\n');
    std::fs::write(&path, text).unwrap();

    let mut events = PanelClient::connect(&backend.socket())
        .unwrap()
        .subscribe(0)
        .unwrap();

    let e = loop {
        match events.recv() {
            Ok(_) => continue,
            Err(e) => break e.to_string(),
        }
    };
    assert!(e.contains("hole"), "and says what is wrong: {e}");
}

#[test]
fn a_backend_that_never_started_is_an_error_that_says_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let e = match PanelClient::connect(&listen::socket_path(dir.path())) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("nothing is listening, so nothing should have connected"),
    };
    assert!(e.contains("carl panel"), "{e}");
}

/// Nothing on screen may claim to be current once the connection has gone.
#[test]
fn a_disconnected_panel_says_so_rather_than_looking_live() {
    let dir = tempfile::tempdir().unwrap();
    army(dir.path());
    let mut backend = Backend::start(dir.path());

    let (live, _) = LivePanel::open(&backend.socket()).unwrap();
    let mut live = live.quiet_after(Duration::from_millis(100));
    assert_eq!(live.health(), Health::Connected);

    backend.down();

    // It has to notice on its own, without being asked and without an event to notice from.
    let mut seen = Vec::new();
    for _ in 0..40 {
        match live.next_update() {
            Update::Health(h) => {
                seen.push(h);
                if h == Health::Disconnected {
                    break;
                }
            }
            other => panic!("nothing was happening, so nothing should have arrived: {other:?}"),
        }
    }
    assert!(seen.contains(&Health::Disconnected), "{seen:?}");
    assert_ne!(live.health(), Health::Connected);
}

/// Two backends on one home would each see half the panel's requests.
#[test]
fn a_second_backend_on_the_same_home_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Backend::start(dir.path());

    let e = match listen::hold(&backend.socket()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("two backends cannot both own one home"),
    };
    assert!(e.contains("already listening"), "{e}");
}

/// Stopping must not leave debris that makes `ls` suggest something is running.
#[test]
fn a_clean_stop_takes_the_socket_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let at = listen::socket_path(dir.path());
    {
        let held = listen::hold(&at).unwrap();
        assert!(at.exists());
        assert!(PanelClient::connect(held.path()).is_ok());
    }
    assert!(!at.exists(), "the socket went with the server that made it");
    assert!(at.parent().unwrap().exists(), "and the directory stayed");
}

/// And a hard stop, which leaves debris by definition, still restarts.
///
/// SIGKILL rather than a hand made socket file. A dropped listener would do as debris, but only
/// racily: `bind` probes a socket in its way with a real connection, and that connection can sit
/// in the backlog and still complete just after the listener closes. Killing a real backend has
/// no such window and is what actually happens.
#[test]
fn debris_from_a_hard_stop_does_not_block_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let at = listen::socket_path(dir.path());

    let mut dead = Backend::start(dir.path());
    dead.down();
    assert!(
        at.exists(),
        "SIGKILL runs no handler, so the socket is left behind"
    );

    // Which the next start has to recognise as debris rather than as a live backend.
    let backend = Backend::start(dir.path());
    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    client.ping().unwrap();

    // Checked while it is running. A dead backend's leftover file would have the right mode too,
    // and proving that would prove nothing.
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(&at).unwrap().permissions().mode() & 0o777
    };
    assert_eq!(mode, 0o600, "and the permissions are right after a restart");
}

/// systemd stops a service with SIGTERM, so this is the ordinary stop rather than the rare one.
///
/// `Child::kill` sends SIGKILL, which by design no handler can catch, so this reaches for the
/// signal the real world sends.
#[test]
fn a_sigterm_takes_the_socket_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Backend::start(dir.path());
    let at = backend.socket();
    assert!(at.exists());

    let pid = backend.child.as_ref().unwrap().id();
    // Safety: a pid this test spawned and has not reaped.
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    backend.child.as_mut().unwrap().wait().unwrap();
    backend.child = None;

    assert!(
        !at.exists(),
        "the socket went with the process it belonged to"
    );
    assert!(at.parent().unwrap().exists(), "and the directory stayed");
}

/// A restart after a clean stop is the ordinary case and must need no cleanup.
#[test]
fn a_clean_stop_and_a_restart_both_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut backend = Backend::start(dir.path());

    for _ in 0..3 {
        let mut client = PanelClient::connect(&backend.socket()).unwrap();
        client.ping().unwrap();
        backend.down();
        backend.up();
    }
    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    client.ping().unwrap();
}
