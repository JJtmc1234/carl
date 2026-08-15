//! The workspace facade, including the security properties it has to keep.

use super::*;
use crate::providers::Diagnostics;

fn wait_for(w: &mut Workspace, id: SessionId, wanted: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut seen = String::new();
    while std::time::Instant::now() < deadline {
        seen.push_str(&String::from_utf8_lossy(&w.drain(id).unwrap()));
        if seen.contains(wanted) {
            return seen;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("never saw {wanted:?}. Saw: {seen}");
}

fn repository() -> (PathBuf, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("git runs");
        assert!(out.status.success(), "git {args:?}");
    };
    git(&["init", "--quiet", "-b", "main"]);
    git(&["config", "user.email", "t@example.invalid"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("thing.txt"), "one\ntwo\nthree\n").unwrap();
    git(&["add", "thing.txt"]);
    git(&["commit", "--quiet", "-m", "first"]);
    (root, d)
}

// ---- terminal sessions ----

#[test]
fn a_terminal_session_can_be_opened_used_and_closed() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let id = w.open_terminal(d.path(), Size::default()).unwrap();
    assert_eq!(w.terminals(), vec![id]);
    assert!(w.is_alive(id));

    w.input_line(id, "echo through-the-facade").unwrap();
    wait_for(&mut w, id, "through-the-facade");

    w.close_terminal(id).unwrap();
    assert!(w.terminals().is_empty());
    assert!(!w.is_alive(id), "a closed session is not alive");
}

#[test]
fn two_terminals_are_separate_sessions() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let first = w.open_terminal(d.path(), Size::default()).unwrap();
    let second = w.open_terminal(d.path(), Size::default()).unwrap();
    assert_ne!(first, second);

    w.input_line(first, "MARKER=one").unwrap();
    w.input_line(first, "echo VAR=$MARKER").unwrap();
    wait_for(&mut w, first, "VAR=one");

    w.input_line(second, "echo VAR=$MARKER").unwrap();
    let seen = wait_for(&mut w, second, "VAR=");
    assert!(!seen.contains("VAR=one"), "the shells share state: {seen}");
}

#[test]
fn an_unknown_session_is_refused_rather_than_panicking() {
    let mut w = Workspace::new();
    let ghost = SessionId(999);

    assert!(w.input_line(ghost, "echo hello").is_err());
    assert!(w.drain(ghost).is_err());
    assert!(w.resize(ghost, Size::default()).is_err());
    assert!(w.close_terminal(ghost).is_err());
    assert!(w.save(ghost, "x").is_err());
    assert!(w.close_file(ghost).is_err());
    assert_eq!(w.cwd(ghost), None);
    assert_eq!(w.contents(ghost), None);
    assert_eq!(w.file_info(ghost), None);
    assert_eq!(w.changed_on_disk(ghost), None);
    assert!(!w.is_alive(ghost));
}

/// The security property, through the facade this time.
#[test]
fn a_terminal_opened_through_the_facade_is_still_only_this_user() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(d.path(), Size::default()).unwrap();

    let mine = unsafe { libc::getuid() };
    w.input_line(id, "echo UID=$(id -u) EUID=$(id -ur)")
        .unwrap();
    let seen = wait_for(&mut w, id, &format!("UID={mine} EUID={mine}"));

    assert!(seen.contains(&format!("UID={mine} EUID={mine}")), "{seen}");
    assert_ne!(mine, 0, "this proves nothing when run as the superuser");
}

/// A closed panel must not leave shells behind, whether or not close was called.
#[test]
fn dropping_the_workspace_closes_every_terminal() {
    let d = tempfile::tempdir().unwrap();
    let pids: Vec<u32> = {
        let mut w = Workspace::new();
        let a = w.open_terminal(d.path(), Size::default()).unwrap();
        let b = w.open_terminal(d.path(), Size::default()).unwrap();
        vec![
            w.terminals.get(&a).unwrap().pid().unwrap(),
            w.terminals.get(&b).unwrap().pid().unwrap(),
        ]
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if pids
            .iter()
            .all(|p| crate::providers::system::process::read(*p).is_none())
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("shells outlived the workspace: {pids:?}");
}

#[test]
fn the_working_directory_is_reported_through_the_facade() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().canonicalize().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(&real, Size::default()).unwrap();

    assert_eq!(w.cwd(id).as_deref(), Some(real.as_path()));
    let _ = w.drain(id);
}

/// Bounded output stays bounded when driven through the facade.
#[test]
fn a_flood_of_output_stays_bounded() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(d.path(), Size::default()).unwrap();

    w.input_line(
        id,
        "for i in $(seq 1 20000); do echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done",
    )
    .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        let _ = w.drain(id);
        let held = w.terminals.get(&id).unwrap().scrollback().len();
        if held >= super::super::terminal::SCROLLBACK_BYTES {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let held = w.terminals.get(&id).unwrap().scrollback().len();
    assert!(
        held <= super::super::terminal::SCROLLBACK_BYTES,
        "scrollback grew to {held} bytes"
    );
}

// ---- editor sessions ----

#[test]
fn a_file_can_be_opened_saved_and_described() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("thing.rs");
    std::fs::write(&path, "fn main() {}\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();

    assert_eq!(w.contents(id), Some("fn main() {}\n"));
    let info = w.file_info(id).unwrap();
    assert_eq!(info.extension.as_deref(), Some("rs"));
    assert_eq!(info.lines, 1);
    assert!(!info.read_only);
    assert!(!info.changed_on_disk);

    w.save(id, "fn main() { println!(); }\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "fn main() { println!(); }\n"
    );
    w.close_file(id).unwrap();
    assert!(w.files().is_empty());
}

#[test]
fn a_read_only_file_refuses_to_save_through_the_facade() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("thing.txt");
    std::fs::write(&path, "look\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadOnly).unwrap();
    assert!(w.file_info(id).unwrap().read_only);

    let err = w.save(id, "touched\n").unwrap_err().to_string();
    assert!(err.contains("read only"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "look\n");
}

#[test]
fn a_stale_save_is_refused_and_reload_clears_it() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("thing.txt");
    std::fs::write(&path, "aaaa\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();

    // Same length, same second, which is the case a timestamp alone would miss.
    std::fs::write(&path, "bbbb\n").unwrap();
    assert_eq!(w.changed_on_disk(id), Some(true));
    assert!(w.save(id, "cccc\n").is_err());

    w.reload(id).unwrap();
    assert_eq!(w.contents(id), Some("bbbb\n"));
    w.save(id, "cccc\n").unwrap();
}

// ---- diff ----

#[test]
fn an_open_file_can_be_compared_with_head_and_with_its_buffer() {
    let (root, _d) = repository();
    let mut w = Workspace::new();
    let id = w
        .open_file(root.join("thing.txt"), Mode::ReadWrite)
        .unwrap();

    assert!(
        w.diff_against_head(id).unwrap().is_empty(),
        "nothing changed yet"
    );

    let buffer = w.diff_buffer(id, "one\ntwo\nthree\nfour\n").unwrap();
    assert!(buffer.contains("+four"), "{buffer}");

    w.save(id, "one\nCHANGED\nthree\n").unwrap();
    let against_head = w.diff_against_head(id).unwrap();
    assert!(against_head.contains("+CHANGED"), "{against_head}");
}

#[test]
fn a_file_outside_a_repository_says_so_clearly() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("loose.txt");
    std::fs::write(&path, "x\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();
    let err = w.diff_against_head(id).unwrap_err().to_string();
    assert!(err.contains("not inside a git repository"), "{err}");
}

#[test]
fn a_worktree_can_be_summarised_through_the_facade() {
    let (root, _d) = repository();
    let w = Workspace::new();
    assert_eq!(w.worktree_summary(&root).unwrap(), "no local changes");

    std::fs::write(root.join("thing.txt"), "edited\n").unwrap();
    assert_eq!(w.worktree_summary(&root).unwrap(), "1 changed");
    assert_eq!(w.worktree_changes(&root).unwrap().len(), 1);
}

/// A repository path with spaces has to work, because git is run as an argument list rather
/// than through a shell and this is the test that proves it.
#[test]
fn a_repository_path_with_spaces_works() {
    let d = tempfile::tempdir().unwrap();
    let root = d
        .path()
        .canonicalize()
        .unwrap()
        .join("a project with spaces");
    std::fs::create_dir(&root).unwrap();

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("git runs")
    };
    git(&["init", "--quiet", "-b", "main"]);
    git(&["config", "user.email", "t@example.invalid"]);
    git(&["config", "user.name", "T"]);
    std::fs::write(root.join("a file.txt"), "one\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "first"]);

    let w = Workspace::new();
    assert_eq!(w.worktree_summary(&root).unwrap(), "no local changes");

    std::fs::write(root.join("a file.txt"), "two\n").unwrap();
    assert_eq!(w.worktree_summary(&root).unwrap(), "1 changed");
}

/// A path that looks like a shell command is a path. Git is never handed a shell.
#[test]
fn a_repository_path_that_looks_like_a_command_is_only_a_path() {
    let d = tempfile::tempdir().unwrap();
    let hostile = d.path().join("; touch /tmp/carl-panel-should-not-exist");
    let w = Workspace::new();

    assert!(w.worktree_summary(&hostile).is_err(), "not a repository");
    assert!(
        !std::path::Path::new("/tmp/carl-panel-should-not-exist").exists(),
        "a path was executed as a command"
    );
}

// ---- investigation ----

#[test]
fn a_diagnostic_row_can_be_investigated_by_its_component_id() {
    let d = tempfile::tempdir().unwrap();
    let snapshot = Diagnostics::new(d.path()).snapshot_at(1_000);
    let w = Workspace::new();

    let found = w
        .investigate(&snapshot, "system.memory")
        .expect("memory is in the snapshot");
    assert_eq!(found.component, "system.memory");
    assert_eq!(found.group, "system");
    assert_eq!(found.kind, Kind::Sampled);
    assert!(found.measured_at.is_some());
    assert!(!found.metrics.is_empty());

    let army = w.investigate(&snapshot, "army.journal").unwrap();
    assert_eq!(army.group, "army");
    assert_eq!(army.kind, Kind::EventDriven);
    assert_eq!(army.measured_at, None);
}

/// The whole security question for this feature, answered by it being a map lookup.
#[test]
fn a_hostile_component_string_is_only_ever_a_lookup_key() {
    let d = tempfile::tempdir().unwrap();
    let snapshot = Diagnostics::new(d.path()).snapshot_at(1_000);
    let w = Workspace::new();

    let sentinel = std::path::Path::new("/tmp/carl-panel-investigate-should-not-exist");
    for hostile in [
        "; touch /tmp/carl-panel-investigate-should-not-exist",
        "$(touch /tmp/carl-panel-investigate-should-not-exist)",
        "`touch /tmp/carl-panel-investigate-should-not-exist`",
        "system.memory; rm -rf /",
        "../../etc/passwd",
        "system.memory\0extra",
        "",
    ] {
        assert_eq!(
            w.investigate(&snapshot, hostile),
            None,
            "{hostile:?} matched something"
        );
    }
    assert!(!sentinel.exists(), "a component string was executed");
}

#[test]
fn investigating_something_that_is_not_in_the_snapshot_finds_nothing() {
    let d = tempfile::tempdir().unwrap();
    let snapshot = Diagnostics::new(d.path()).snapshot_at(1_000);
    let w = Workspace::new();
    assert_eq!(w.investigate(&snapshot, "system.tea"), None);
}
