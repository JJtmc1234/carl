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

// ---- lifecycle, for a panel that stays open all day ----

#[test]
fn many_terminals_coexist_and_every_id_is_distinct() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let ids: Vec<SessionId> = (0..5)
        .map(|_| w.open_terminal(d.path(), Size::default()).unwrap())
        .collect();

    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len(), "ids repeated: {ids:?}");
    assert_eq!(w.terminals().len(), 5);
    for id in &ids {
        assert!(w.is_alive(*id));
    }
}

/// An id is never handed out twice, even after the session it named has gone.
#[test]
fn a_closed_session_does_not_give_its_id_back() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let first = w.open_terminal(d.path(), Size::default()).unwrap();
    w.close_terminal(first).unwrap();
    let second = w.open_terminal(d.path(), Size::default()).unwrap();

    assert_ne!(first, second, "a stale handle would address a live session");
}

#[test]
fn closing_one_terminal_leaves_the_others_running() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let a = w.open_terminal(d.path(), Size::default()).unwrap();
    let b = w.open_terminal(d.path(), Size::default()).unwrap();
    let c = w.open_terminal(d.path(), Size::default()).unwrap();

    w.close_terminal(b).unwrap();

    assert!(w.is_alive(a), "a was closed too");
    assert!(!w.is_alive(b));
    assert!(w.is_alive(c), "c was closed too");
    assert_eq!(w.terminals().len(), 2);

    // And the survivors still work.
    w.input_line(a, "echo still-here").unwrap();
    wait_for(&mut w, a, "still-here");
}

/// A shell JJ exits from is dead, and the panel has to notice without being told.
#[test]
fn a_shell_that_exits_on_its_own_is_detected_as_dead() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(d.path(), Size::default()).unwrap();

    w.input_line(id, "exit").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !w.is_alive(id) {
            // Still a known session, just not a living one, so the panel can show that.
            assert!(w.terminals().contains(&id));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the shell is still alive after exit");
}

/// Resizing while output is pouring in is the case that deadlocks a naive implementation.
#[test]
fn a_terminal_can_be_resized_while_it_is_producing_output() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(d.path(), Size::default()).unwrap();

    w.input_line(id, "for i in $(seq 1 4000); do echo line-$i; done")
        .unwrap();

    for step in 0..8 {
        w.resize(
            id,
            Size {
                rows: 24 + step,
                cols: 80 + step,
            },
        )
        .unwrap();
        let _ = w.drain(id);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(w.is_alive(id), "resizing under load killed the shell");
    w.input_line(id, "echo survived").unwrap();
    wait_for(&mut w, id, "survived");
}

#[test]
fn a_terminal_can_start_in_a_directory_with_spaces() {
    let d = tempfile::tempdir().unwrap();
    let spaced = d
        .path()
        .canonicalize()
        .unwrap()
        .join("a directory with spaces");
    std::fs::create_dir(&spaced).unwrap();

    let mut w = Workspace::new();
    let id = w.open_terminal(&spaced, Size::default()).unwrap();

    assert_eq!(w.cwd(id).as_deref(), Some(spaced.as_path()));
    w.input_line(id, "echo HERE=$PWD").unwrap();
    let seen = wait_for(&mut w, id, "a directory with spaces");
    assert!(seen.contains("a directory with spaces"), "{seen}");
}

// ---- editor lifecycle ----

#[test]
fn many_files_are_open_at_once_and_stay_separate() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let mut ids = Vec::new();
    for n in 0..4 {
        let path = d.path().join(format!("file{n}.txt"));
        std::fs::write(&path, format!("contents {n}\n")).unwrap();
        ids.push(w.open_file(&path, Mode::ReadWrite).unwrap());
    }

    assert_eq!(w.files().len(), 4);
    for (n, id) in ids.iter().enumerate() {
        assert_eq!(w.contents(*id), Some(format!("contents {n}\n").as_str()));
    }

    // Saving one leaves the others alone.
    w.save(ids[1], "changed\n").unwrap();
    assert_eq!(w.contents(ids[0]), Some("contents 0\n"));
    assert_eq!(w.contents(ids[2]), Some("contents 2\n"));

    w.close_file(ids[1]).unwrap();
    assert_eq!(w.files().len(), 3);
    assert_eq!(w.contents(ids[1]), None);
}

/// A file renamed out from under the editor is gone, and saving must not recreate it at the
/// old name as if nothing happened.
#[test]
fn a_file_renamed_underneath_is_treated_as_changed() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("before.txt");
    std::fs::write(&path, "contents\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();

    std::fs::rename(&path, d.path().join("after.txt")).unwrap();

    assert_eq!(w.changed_on_disk(id), Some(true));
    assert!(
        w.save(id, "mine\n").is_err(),
        "a save resurrected the old name"
    );
    assert!(!path.exists(), "the old path came back");
}

#[test]
fn a_file_deleted_underneath_refuses_to_be_saved_over() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("doomed.txt");
    std::fs::write(&path, "contents\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();
    std::fs::remove_file(&path).unwrap();

    assert_eq!(w.changed_on_disk(id), Some(true));
    assert!(w.save(id, "mine\n").is_err());
    assert!(w.reload(id).is_err(), "there is nothing to reload");
}

#[test]
fn a_rapid_same_size_external_edit_is_caught() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("racy.txt");
    std::fs::write(&path, "aaaaaaaa\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();

    // Same byte count, same second, different content. Only the hash catches this.
    std::fs::write(&path, "bbbbbbbb\n").unwrap();

    assert_eq!(w.changed_on_disk(id), Some(true));
    let err = w.save(id, "cccccccc\n").unwrap_err().to_string();
    assert!(err.contains("changed on disk"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "bbbbbbbb\n");
}

#[test]
fn a_file_with_spaces_in_its_path_opens_and_saves() {
    let d = tempfile::tempdir().unwrap();
    let folder = d.path().join("some folder");
    std::fs::create_dir(&folder).unwrap();
    let path = folder.join("a file.txt");
    std::fs::write(&path, "one\n").unwrap();

    let mut w = Workspace::new();
    let id = w.open_file(&path, Mode::ReadWrite).unwrap();
    assert_eq!(w.contents(id), Some("one\n"));

    w.save(id, "two\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "two\n");
    assert!(w.file_info(id).unwrap().path.ends_with("a file.txt"));
}

// ---- leak audit ----

/// Every pid this process still has as a child, from /proc.
fn living(pids: &[u32]) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|p| crate::providers::system::process::read(*p).is_some())
        .collect()
}

fn gone_within(pids: &[u32], patience: std::time::Duration) -> Vec<u32> {
    let deadline = std::time::Instant::now() + patience;
    while std::time::Instant::now() < deadline {
        if living(pids).is_empty() {
            return Vec::new();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    living(pids)
}

/// The leak that would matter: a panel opening and closing terminals all day.
#[test]
fn twenty_open_and_close_cycles_leave_nothing_behind() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let mut pids = Vec::new();

    for round in 0..20 {
        let id = w.open_terminal(d.path(), Size::default()).unwrap();
        pids.push(w.terminals.get(&id).unwrap().pid().unwrap());
        w.input_line(id, "echo round").unwrap();
        w.close_terminal(id).unwrap();
        assert_eq!(w.held().0, 0, "a session was kept after round {round}");
    }

    let leaked = gone_within(&pids, std::time::Duration::from_secs(10));
    assert!(leaked.is_empty(), "shells left running: {leaked:?}");
    assert_eq!(w.terminals().len(), 0);
}

/// Sessions that die on their own must not pile up in the table for the life of the process.
#[test]
fn dead_sessions_are_reaped_rather_than_accumulating() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let mut pids = Vec::new();

    for _ in 0..10 {
        let id = w.open_terminal(d.path(), Size::default()).unwrap();
        pids.push(w.terminals.get(&id).unwrap().pid().unwrap());
        w.input_line(id, "exit").unwrap();
    }
    assert_eq!(w.held().0, 10, "all ten are still known while they die");

    // They are kept until somebody asks, so a panel can show that they exited.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while std::time::Instant::now() < deadline {
        if w.reap().len() + w.held().0 == 10 && w.held().0 == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert_eq!(w.held().0, 0, "dead sessions were never cleared");
    assert!(w.reap().is_empty(), "reaping twice found more");
    let leaked = gone_within(&pids, std::time::Duration::from_secs(5));
    assert!(
        leaked.is_empty(),
        "reaped sessions left processes: {leaked:?}"
    );
}

/// Reaping must not touch a session that is still working.
#[test]
fn reaping_leaves_the_living_alone() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let alive = w.open_terminal(d.path(), Size::default()).unwrap();
    let doomed = w.open_terminal(d.path(), Size::default()).unwrap();
    w.input_line(doomed, "exit").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let reaped = w.reap();
        if reaped.contains(&doomed) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the dead session was never reaped"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    assert_eq!(w.terminals(), vec![alive], "the living one was reaped too");
    w.input_line(alive, "echo still-working").unwrap();
    wait_for(&mut w, alive, "still-working");
}

/// Files are held until released, and releasing them frees the table.
#[test]
fn file_sessions_are_released_when_closed() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();

    let mut ids = Vec::new();
    for n in 0..25 {
        let path = d.path().join(format!("f{n}.txt"));
        std::fs::write(&path, "x\n").unwrap();
        ids.push(w.open_file(&path, Mode::ReadWrite).unwrap());
    }
    assert_eq!(w.held().1, 25);

    for id in ids {
        w.close_file(id).unwrap();
    }
    assert_eq!(w.held().1, 0, "file sessions were not released");
    assert!(w.files().is_empty());
}

/// Investigation holds nothing, so clicking a row a thousand times costs nothing.
#[test]
fn repeated_investigation_accumulates_no_state() {
    let d = tempfile::tempdir().unwrap();
    let snapshot = Diagnostics::new(d.path()).snapshot_at(1_000);
    let w = Workspace::new();

    let before = w.held();
    let first = w.investigate(&snapshot, "system.memory").unwrap();
    for _ in 0..1_000 {
        let again = w.investigate(&snapshot, "system.memory").unwrap();
        assert_eq!(again, first, "investigation is not a pure lookup");
        assert_eq!(w.investigate(&snapshot, "nothing.here"), None);
    }
    assert_eq!(w.held(), before, "investigating opened a session");
    assert_eq!(w.held(), (0, 0));
}

/// Output stays bounded across many drains rather than only on the first.
#[test]
fn bounded_output_stays_bounded_over_a_long_session() {
    let d = tempfile::tempdir().unwrap();
    let mut w = Workspace::new();
    let id = w.open_terminal(d.path(), Size::default()).unwrap();

    for _ in 0..3 {
        w.input_line(
            id,
            "for i in $(seq 1 8000); do echo aaaaaaaaaaaaaaaaaaaaaaaaaaaa; done",
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            let _ = w.drain(id);
            if w.terminals.get(&id).unwrap().scrollback().len()
                >= super::super::terminal::SCROLLBACK_BYTES
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let held = w.terminals.get(&id).unwrap().scrollback().len();
        assert!(
            held <= super::super::terminal::SCROLLBACK_BYTES,
            "scrollback reached {held} bytes"
        );
    }
}
