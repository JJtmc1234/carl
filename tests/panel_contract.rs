//! The contract Process 2's UI bridge has to satisfy, proved end to end in one scenario.
//!
//! Everything here runs against the real binary on a real socket, because the questions are
//! about two processes: what a subscriber sees while the army is quiet, what happens to the
//! resume point when telemetry arrives, and whether an event that happened while nobody was
//! connected comes back exactly once.
//!
//! **The property that ties it together.** A journal sequence is a place in the record. A
//! telemetry frame is a number that was true when somebody looked. They travel on the same
//! socket and must never be confused, because a panel that let telemetry move its resume point
//! would reconnect asking to continue from a number the journal never issued, and the backend
//! would answer honestly that it cannot, and the panel would resnapshot forever.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carl::army::event::{Event, Journal};
use carl::army::personnel::found;
use carl::army::task::{Task, Verification};
use carl::panel::client::{Incoming, PanelClient};
use carl::panel::listen;

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

    fn up(&mut self) {
        self.child = Some(
            std::process::Command::new(env!("CARGO_BIN_EXE_carl"))
                .arg("--home")
                .arg(&self.home)
                .arg("panel")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("starting carl panel"),
        );
        for _ in 0..400 {
            if PanelClient::connect(&self.socket()).is_ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend never came up");
    }

    fn socket(&self) -> PathBuf {
        listen::socket_path(&self.home)
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

/// Writes one delegation and returns the sequence it landed at.
fn one_army_event(journal: &mut Journal, goal: &str) -> u64 {
    let t = Task::assign("mason", "nora", goal, verification()).unwrap();
    journal
        .append(
            "mason",
            Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: None,
                must: t.verification.must.clone(),
                project: None,
                workspace: None,
                objective: None,
            },
        )
        .unwrap()
        .seq
}

/// The whole thing, in the order a UI will actually meet it.
///
/// snapshot at N, telemetry with N unchanged, army event N+1, more telemetry, disconnect, an
/// army event while away, reconnect, N+2 replayed exactly once, telemetry still flowing.
#[test]
fn the_backend_contract_a_ui_bridge_has_to_satisfy() {
    let dir = tempfile::tempdir().unwrap();
    let people = found(dir.path(), 1).unwrap();
    let backend = Backend::start(dir.path());
    let mut journal = Journal::open(people.journal_path()).unwrap();

    // 1. Snapshot at N.
    let mut client = PanelClient::connect(&backend.socket()).unwrap();
    let snapshot = client.snapshot().unwrap();
    let n = snapshot.seq;
    assert!(n > 0, "founding wrote records, so N is a real sequence");

    // 2. Subscribe from exactly it. Nothing is replayed, because nothing has happened.
    let mut events = PanelClient::connect(&backend.socket())
        .unwrap()
        .subscribe(n)
        .unwrap();
    match events.recv().unwrap() {
        Incoming::CaughtUp { seq } => assert_eq!(seq, n, "the snapshot joins the stream exactly"),
        other => panic!("expected to be caught up already: {other:?}"),
    }

    // 3. Telemetry arrives on its own, and does not move the resume point.
    let mut telemetry_seen = 0;
    let mut at_first = 0;
    while telemetry_seen == 0 {
        match events.recv().unwrap() {
            Incoming::Telemetry { at, diagnostics } => {
                telemetry_seen += 1;
                at_first = at;
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d.kind == carl::providers::Kind::Sampled),
                    "only sampled telemetry travels this way"
                );
                assert!(
                    diagnostics.iter().all(|d| d.measured_at.is_some()),
                    "and each one knows when it was read"
                );
                assert_eq!(events.last_seq(), n, "telemetry moved the resume point");
            }
            other => panic!("the army is quiet, so nothing else should arrive: {other:?}"),
        }
    }

    // 4. An army event. It is the next sequence and nothing about telemetry disturbed that.
    let n1 = one_army_event(&mut journal, "the first thing");
    assert_eq!(n1, n + 1, "the journal numbers on its own terms");

    let mut got = None;
    while got.is_none() {
        match events.recv().unwrap() {
            Incoming::Event(e) => got = Some(e.seq),
            Incoming::Telemetry { at, .. } => {
                assert!(at >= at_first, "samples do not go backwards");
                assert_eq!(events.last_seq(), n, "still not moved by telemetry");
            }
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(got, Some(n + 1));
    assert_eq!(events.last_seq(), n + 1, "and an event does move it");

    // 5. More telemetry after the event, so the two genuinely interleave.
    match events.recv().unwrap() {
        Incoming::Telemetry { .. } => {}
        Incoming::Event(e) => panic!("nothing else happened: {}", e.seq),
        other => panic!("unexpected: {other:?}"),
    }
    assert_eq!(events.last_seq(), n + 1);

    // 6. Disconnect, and work carries on. The army does not stop because nobody is watching.
    let resume_from = events.last_seq();
    drop(events);
    let n2 = one_army_event(&mut journal, "while nobody was connected");
    assert_eq!(n2, n + 2);

    // 7. Reconnect from exactly where it got to.
    let mut events = PanelClient::connect(&backend.socket())
        .unwrap()
        .subscribe(resume_from)
        .unwrap();

    let mut replayed = Vec::new();
    loop {
        match events.recv().unwrap() {
            Incoming::Event(e) => replayed.push(e.seq),
            Incoming::CaughtUp { seq } => {
                assert_eq!(seq, n2);
                break;
            }
            Incoming::Telemetry { .. } => continue,
            other => panic!("unexpected: {other:?}"),
        }
    }
    assert_eq!(
        replayed,
        vec![n + 2],
        "exactly once, and nothing before it again"
    );

    // 8. And telemetry keeps flowing on the new connection.
    match events.recv().unwrap() {
        Incoming::Telemetry { at, .. } => {
            assert!(at > 0);
            assert_eq!(
                events.last_seq(),
                n2,
                "still the journal's number and no other"
            );
        }
        other => panic!("the army is quiet again: {other:?}"),
    }
}

/// Looking at a home must not change what is in it.
///
/// A real panel was opened on JJ's home and left an `army/` directory behind, which is the
/// difference between "no army has been founded here" and "an army was founded and everybody
/// left". `panel/` is different and is fine: the socket has to live somewhere, and a backend
/// that is running is a true thing to have written down.
#[test]
fn observing_an_unfounded_army_does_not_bring_one_into_existence() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();

    assert!(!home.join("army").exists(), "nothing here to begin with");

    let backend = Backend::start(home);
    let mut client = PanelClient::connect(&backend.socket()).unwrap();

    let snapshot = client.snapshot().unwrap();
    client.ping().unwrap();
    client.snapshot().unwrap();
    client
        .command(carl::panel::PanelCommand::Inspect {
            agent: "nora".into(),
        })
        .unwrap();

    // The organisation is compiled in, so it is still fully described. What is absent is any
    // claim that these agents have state, which is the honest answer.
    // Everybody in the table except JJ, who is the human and has no folder to be missing.
    // Counted from the table rather than written down, so growing the organisation does not
    // silently make this assertion about a number nobody meant. By rank rather than by name,
    // so a second human does not quietly break it.
    let agents = carl::army::org::everyone()
        .iter()
        .filter(|a| a.rank != carl::army::org::Rank::Human)
        .count();
    assert_eq!(
        snapshot.agents.len(),
        agents,
        "the table is still the table"
    );
    assert!(
        snapshot.agents.iter().all(|a| !a.enlisted),
        "and none of them has a folder"
    );

    assert!(
        !home.join("army").exists(),
        "reading created an army directory"
    );
    assert!(
        !home.join("run").exists(),
        "reading created a journal directory"
    );
    assert!(
        home.join("panel").exists(),
        "the socket has to live somewhere, and this one is legitimate"
    );
}

/// And founding still does create it, so the fix took nothing away.
#[test]
fn founding_still_creates_the_army_directory() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!dir.path().join("army").exists());

    found(dir.path(), 1).unwrap();
    assert!(dir.path().join("army").exists());
    assert!(dir.path().join("army/nora").exists(), "with folders in it");
}

/// A shell that exited must still be there to be drawn, until somebody clears it.
///
/// The provider's own reap test proves dead sessions are cleared and leak no processes, and its
/// comment says they are kept until asked, but nothing checked that half: it reaps immediately.
/// This is the half the UI depends on. A terminal row that vanishes the instant its shell exits
/// never gets to say why it exited, and JJ is left with a panel that had a terminal a moment ago
/// and now does not.
#[test]
fn a_dead_terminal_is_still_there_to_be_drawn_until_it_is_reaped() {
    use carl::providers::workspace::Workspace;
    use carl::providers::workspace::terminal::Size;

    let dir = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::new();

    let alive = workspace
        .open_terminal(dir.path(), Size::default())
        .unwrap();
    let doomed = workspace
        .open_terminal(dir.path(), Size::default())
        .unwrap();
    assert_eq!(workspace.held().0, 2);

    workspace.input_line(doomed, "exit").unwrap();

    // Wait for the shell to actually be gone, without reaping on the way.
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if !workspace.terminals().contains(&doomed) {
            panic!("the dead session disappeared before anybody reaped it");
        }
        if !workspace.is_alive(doomed) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    assert!(
        !workspace.is_alive(doomed),
        "the shell never exited, so this proves nothing"
    );
    assert_eq!(
        workspace.held().0,
        2,
        "a dead session is still held, so a panel can draw that it died"
    );
    assert!(workspace.terminals().contains(&doomed));

    // And only now does it go.
    let reaped = workspace.reap();
    assert_eq!(reaped, vec![doomed]);
    assert_eq!(workspace.held().0, 1, "and the living one is untouched");
    assert!(workspace.is_alive(alive));
}
