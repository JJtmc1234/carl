//! The terminal, against a real interactive shell.
//!
//! Nothing here is mocked. A pseudoterminal that works against a fake is a pseudoterminal
//! nobody has evidence about, and the whole reason this module exists rather than a command
//! runner is behaviour a fake cannot reproduce.

use super::*;

/// Reads until `wanted` appears or the patience runs out.
///
/// Polling rather than a fixed sleep, so the test is quick when the shell is quick and does not
/// fail on a loaded machine.
fn wait_for(t: &mut Terminal, wanted: &str) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut seen = String::new();
    while std::time::Instant::now() < deadline {
        seen.push_str(&String::from_utf8_lossy(&t.drain()));
        if seen.contains(wanted) {
            return seen;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("never saw {wanted:?}. Saw: {seen}");
}

fn open_in(dir: &std::path::Path) -> Terminal {
    Terminal::open(dir, Size::default()).expect("a shell should start")
}

#[test]
fn a_shell_starts_and_answers() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());
    assert!(t.is_alive());

    t.send_line("echo hello-from-the-panel").unwrap();
    let seen = wait_for(&mut t, "hello-from-the-panel");
    assert!(seen.contains("hello-from-the-panel"), "{seen}");
}

/// The thing a command runner cannot do. State set by one input has to survive to the next.
#[test]
fn it_is_a_session_rather_than_a_series_of_commands() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("MARKER=persisted").unwrap();
    t.send_line("echo VAR=$MARKER").unwrap();
    let seen = wait_for(&mut t, "VAR=persisted");
    assert!(
        seen.contains("VAR=persisted"),
        "shell state was lost: {seen}"
    );
}

/// A real pseudoterminal, which is what makes interactive programs behave.
#[test]
fn the_shell_is_attached_to_a_real_terminal_device() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("tty").unwrap();
    let seen = wait_for(&mut t, "/dev/pts/");
    assert!(seen.contains("/dev/pts/"), "not a real tty: {seen}");
}

/// The security property, checked rather than asserted in prose.
#[test]
fn the_shell_runs_as_this_user_and_gains_nothing() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    let mine = unsafe { libc::getuid() };
    t.send_line("echo UID=$(id -u) EUID=$(id -ur)").unwrap();

    // Waiting for the resolved number rather than the prefix. A terminal echoes what was
    // typed, so `UID=` appears in the echo of the command before any answer exists, and
    // waiting for that would pass without the shell having run anything.
    let seen = wait_for(&mut t, &format!("UID={mine} EUID={mine}"));
    assert!(
        seen.contains(&format!("UID={mine} EUID={mine}")),
        "the terminal is not running as this user: {seen}"
    );
    assert_ne!(
        mine, 0,
        "this test would prove nothing if run as the superuser"
    );
}

#[test]
fn the_working_directory_is_where_it_was_asked_to_start() {
    let d = tempfile::tempdir().unwrap();
    // The temporary directory may be behind a symlink, so compare what the kernel resolves.
    let real = d.path().canonicalize().unwrap();
    let mut t = open_in(&real);

    assert_eq!(t.started_in(), real);
    let now = t.current_dir().expect("the shell has a working directory");
    assert_eq!(now, real, "started somewhere else");

    let _ = t.drain();
}

/// The panel shows a live working directory, so it has to follow JJ around.
#[test]
fn the_working_directory_follows_a_cd() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().canonicalize().unwrap();
    let inner = real.join("inner");
    std::fs::create_dir(&inner).unwrap();

    let mut t = open_in(&real);
    t.send_line("cd inner").unwrap();
    t.send_line("echo MOVED").unwrap();
    wait_for(&mut t, "MOVED");

    // The shell reports the move once it has processed it, which may lag the echo slightly.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if t.current_dir().as_deref() == Some(inner.as_path()) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!(
        "the working directory did not follow, still {:?}",
        t.current_dir()
    );
}

#[test]
fn a_terminal_can_be_resized() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.resize(Size {
        rows: 50,
        cols: 132,
    })
    .unwrap();
    t.send_line("echo SIZE=$(tput lines)x$(tput cols)").unwrap();
    // The resolved size, not the prefix, for the same reason as the user id test.
    let seen = wait_for(&mut t, "SIZE=50x132");
    assert!(
        seen.contains("SIZE=50x132"),
        "the shell did not see the resize: {seen}"
    );
}

#[test]
fn closing_ends_the_shell() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());
    let pid = t.pid().expect("a shell has a process id");
    assert!(t.is_alive());

    t.close().unwrap();
    assert!(!t.is_alive());
    assert!(
        crate::providers::system::process::read(pid).is_none(),
        "the shell was left behind"
    );
}

/// A closed panel must not leak a shell, whether or not anybody called close.
#[test]
fn dropping_a_terminal_leaves_no_shell_behind() {
    let d = tempfile::tempdir().unwrap();
    let pid = {
        let t = open_in(d.path());
        t.pid().expect("a shell has a process id")
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        if crate::providers::system::process::read(pid).is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the shell outlived the terminal that owned it");
}

#[test]
fn a_shell_that_exits_is_no_longer_alive() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());
    t.send_line("exit").unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if !t.is_alive() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the shell is still running after exit");
}

#[test]
fn a_terminal_cannot_start_in_something_that_is_not_a_directory() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("a-file");
    std::fs::write(&file, "x").unwrap();

    assert!(Terminal::open(&file, Size::default()).is_err());
    assert!(Terminal::open(d.path().join("nope"), Size::default()).is_err());
}

/// A runaway process should cost a fixed amount of memory rather than all of it.
#[test]
fn scrollback_is_bounded() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("for i in $(seq 1 20000); do echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa; done")
        .unwrap();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        t.drain();
        if t.scrollback().len() >= SCROLLBACK_BYTES {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(
        t.scrollback().len() <= SCROLLBACK_BYTES,
        "scrollback grew to {} bytes",
        t.scrollback().len()
    );
}

/// Draining returns what is new, not everything again, or the panel would redraw the whole
/// session on every frame.
#[test]
fn draining_returns_only_what_is_new() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("echo first-line").unwrap();
    wait_for(&mut t, "first-line");

    let again = t.drain();
    assert!(
        !String::from_utf8_lossy(&again).contains("first-line"),
        "the same output came back twice"
    );
}

/// The scrub list is the reason a terminal started from inside a snap does not hand the shell
/// a broken loader. Losing an entry would be silent and would only show up as system binaries
/// dying with an undefined symbol, so the list itself is guarded.
#[test]
fn the_environment_scrub_list_is_intact() {
    for must_go in [
        "LD_LIBRARY_PATH",
        "LD_PRELOAD",
        "SNAP",
        "SNAP_NAME",
        "SNAP_REVISION",
        "GTK_PATH",
        "GIO_MODULE_DIR",
        "GSETTINGS_SCHEMA_DIR",
    ] {
        assert!(
            SCRUB.contains(&must_go),
            "{must_go} was dropped from the scrub list"
        );
    }
}

/// And the environment this module sets does reach the shell, which is what makes the removals
/// above meaningful rather than decorative.
#[test]
fn the_environment_this_module_sets_reaches_the_shell() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("echo TERM_IS=$TERM").unwrap();
    let seen = wait_for(&mut t, "TERM_IS=xterm-256color");
    assert!(seen.contains("TERM_IS=xterm-256color"), "{seen}");
}

/// The removal itself, checked on the builder before anything is spawned.
///
/// Its neighbour above asserts that `SCRUB` still lists the right names, which is a different
/// claim and does not cover this one: delete the removal from `scrub` and the list assertion
/// still passes while every variable leaks into JJ's shell. Both are kept because they guard
/// different things, one that the list has not been thinned and one that the loop still runs.
///
/// No environment is mutated to do this. `CommandBuilder` holds its own map seeded from the
/// base environment, so a removal is observable in process, which is what makes this testable
/// without `set_var` and without racing the other tests spawning shells in parallel.
#[test]
fn the_scrubbed_variables_are_actually_removed_from_the_child_environment() {
    let mut cmd = CommandBuilder::new("/bin/sh");

    // Put every one of them in, as an inherited environment would.
    for name in SCRUB {
        cmd.env(name, "/tmp/should-not-reach-the-shell");
    }
    for name in SCRUB {
        assert!(
            cmd.get_env(name).is_some(),
            "{name} was not set, so removing it would prove nothing"
        );
    }

    scrub(&mut cmd);

    for name in SCRUB {
        assert_eq!(
            cmd.get_env(name),
            None,
            "{name} survived the scrub and would reach the shell"
        );
    }
}

/// And the variable the same function sets does reach the child, so a scrub that simply
/// emptied the environment would not pass.
#[test]
fn scrubbing_keeps_what_it_is_supposed_to_set() {
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.env("KEEP_ME", "yes");
    cmd.env("LD_PRELOAD", "/tmp/evil.so");

    scrub(&mut cmd);

    assert_eq!(cmd.get_env("LD_PRELOAD"), None, "the dangerous one went");
    assert_eq!(
        cmd.get_env("TERM")
            .map(|t| t.to_string_lossy().into_owned()),
        Some("xterm-256color".to_string()),
        "the one it sets is there"
    );
    assert_eq!(
        cmd.get_env("KEEP_ME")
            .map(|t| t.to_string_lossy().into_owned()),
        Some("yes".to_string()),
        "an unrelated variable was removed, so the scrub is too broad"
    );
}

/// The cap has to hold while nobody is draining, which is the whole of bug 13.
///
/// `scrollback_is_bounded` above drains in a loop, and a caller that drains continuously was
/// exactly the only caller the old bound was true for. The reader thread pushed every chunk
/// into an unbounded channel, and the trim only ran once `drain` had moved bytes out of it, so
/// the real cost was whatever the shell printed between two drains.
///
/// Twenty megabytes rather than `yes` on its own, so the command ends by itself and cannot
/// leave a spinning process behind if this test fails.
#[test]
fn output_is_bounded_even_when_nobody_drains() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("yes hello | head -c 20000000").unwrap();
    // Deliberately not draining. This is the panel drawing some other pane.
    std::thread::sleep(std::time::Duration::from_secs(2));

    let held = t.scrollback().len();
    let fresh = t.drain();
    let produced = fresh.len() as u64 + t.dropped();

    // Otherwise the shell was too slow to have proved anything and the assertions below would
    // pass on the old code as well.
    assert!(
        produced > 4 * SCROLLBACK_BYTES as u64,
        "only {produced} bytes came through, which is too few to test a {SCROLLBACK_BYTES} cap"
    );
    assert!(held <= SCROLLBACK_BYTES, "scrollback held {held} bytes");
    assert!(
        fresh.len() <= SCROLLBACK_BYTES,
        "one drain returned {} bytes against a {SCROLLBACK_BYTES} cap",
        fresh.len()
    );
    assert!(t.dropped() > 0, "bytes were dropped and it did not say so");
}

/// Trimming the front moves the undrained mark with it, or the next drain reads the wrong
/// window. Cheap to get wrong and silent when it is.
///
/// Worth being clear about what this is: a guard on the arithmetic the fix introduced, not a
/// regression guard for bug 13. It passes against the old code, because there was no mark to
/// keep in step back when the trim only ran at drain time. The three that do fail against the
/// old code are `output_is_bounded_even_when_nobody_drains`,
/// `a_pane_that_never_stops_printing_stays_bounded` and
/// `trimming_lands_on_a_character_boundary`.
#[test]
fn trimming_the_front_does_not_confuse_the_next_drain() {
    let d = tempfile::tempdir().unwrap();
    let mut t = open_in(d.path());

    t.send_line("yes hello | head -c 2000000").unwrap();
    std::thread::sleep(std::time::Duration::from_secs(1));

    // The scrollback is the last SCROLLBACK_BYTES, and an undrained reader is owed exactly
    // that same window rather than more of it or a slice of the wrong part.
    let held = t.scrollback();
    let fresh = t.drain();
    assert_eq!(fresh, held, "the drain did not match what was held");

    // And a second drain is empty, because everything has now been handed over once.
    t.send_line("echo done-here").unwrap();
    let after = wait_for(&mut t, "done-here");
    assert!(
        !after.contains("hello"),
        "output came back a second time after being drained"
    );
}
