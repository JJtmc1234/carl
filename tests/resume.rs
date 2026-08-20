//! Whether a conversation is resumed, checked against what `claude` is actually launched with.
//!
//! Bug 17. The session id is minted and written to the registry before `claude` is ever
//! contacted, and the resume decision was made from whether the registry had an entry. So a
//! thread whose very first turn failed kept an entry naming an id nothing on the far side had
//! ever seen, and every later turn asked to resume it. The real binary answers that with "No
//! conversation found with session ID" and exits 1, so the thread was wedged permanently and
//! only a hand edit of threads.json cleared it.
//!
//! The stand in records its own argument vector and then fails, which is the only way to see
//! the decision. Asserting on the flag Carl computes would be asserting that a function agrees
//! with itself.

use std::io::Write;
use std::path::{Path, PathBuf};

use carl::claude::{Flow, Pool, Runner};
use carl::{Registry, SessionId, ThreadId};

/// A `claude` that writes down how it was called and then dies.
///
/// Dying is the point. It puts the pool on its reopen and retry path, which is where the
/// built in recovery used to reach for the same dead session id.
fn recording_claude(dir: &Path, log: &Path) -> PathBuf {
    let path = dir.join("recording-claude");
    let script = format!(
        "#!/bin/bash\n\
         printf '%s\\n' \"$*\" >> {}\n\
         read -r line\n\
         exit 1\n",
        log.display()
    );

    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    f.sync_all().unwrap();
    drop(f);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Every way the stand in was launched, in order.
fn launches(dir: &Path, session: &SessionId, resume: bool) -> Vec<String> {
    let log = dir.join("argv.log");

    // Retried for the same reason `tests/session.rs` retries. Between a fork and its exec the
    // child holds every open descriptor, including another test thread's handle on the script
    // it is still writing, and Linux refuses to exec a file anybody has open for writing.
    for _ in 0..20 {
        let _ = std::fs::remove_file(&log);
        let mut pool = Pool::new(Runner::at(recording_claude(dir, &log)), dir, "you are carl");
        let _ = pool.ask(
            &ThreadId::new("slack-C1-1700").unwrap(),
            session,
            resume,
            "anything",
            &mut |_| Flow::Continue,
            &mut || Flow::Continue,
        );
        let seen = std::fs::read_to_string(&log).unwrap_or_default();
        if !seen.trim().is_empty() {
            return seen.lines().map(str::to_string).collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    panic!("the stand in never ran");
}

/// A first turn that fails must not turn into a resume, and the retry must not either.
#[test]
fn a_thread_with_no_completed_turn_is_never_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionId::fresh().unwrap();

    let seen = launches(dir.path(), &session, false);

    assert!(
        seen.len() >= 2,
        "the pool did not reopen and retry, so the retry is untested: {seen:?}"
    );
    for (n, args) in seen.iter().enumerate() {
        assert!(
            !args.contains("--resume"),
            "launch {} asked to resume a session claude never created: {args}",
            n + 1
        );
        assert!(
            args.contains(&session.to_string()),
            "launch {} did not pin the minted id: {args}",
            n + 1
        );
    }
}

/// And a thread that has genuinely talked before is still resumed, both times.
///
/// The other direction matters as much. Refusing to resume a real conversation would start a
/// second one silently, which is the failure the resume flag exists to prevent.
///
/// This one passes against the old code too, since the old code resumed in every case. It
/// guards the fix against overreaching rather than guarding the bug. The two that fail against
/// the old code are `a_thread_with_no_completed_turn_is_never_resumed` and
/// `a_registry_entry_alone_is_not_a_conversation`.
#[test]
fn a_thread_that_has_talked_before_is_still_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let session = SessionId::fresh().unwrap();

    let seen = launches(dir.path(), &session, true);

    assert!(seen.len() >= 2, "{seen:?}");
    for (n, args) in seen.iter().enumerate() {
        assert!(
            args.contains("--resume"),
            "launch {} started a second conversation instead of resuming: {args}",
            n + 1
        );
    }
}

/// The registry's own answer to the question, which is what decides the flag above.
#[test]
fn a_registry_entry_alone_is_not_a_conversation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("threads.json");
    let thread = ThreadId::new("slack-C1-1700").unwrap();

    let mut registry = Registry::open(&path).unwrap();
    let (minted, is_new) = registry.session_for(&thread, 100).unwrap();
    assert!(is_new);
    assert!(
        !registry.has_transcript(&thread),
        "the id is minted before claude is contacted, so there is nothing to resume yet"
    );

    // The turn fails, so `record_turn` never runs, and a restart reads the same file. The
    // entry is present, which is exactly what the old decision was made on.
    let reopened = Registry::open(&path).unwrap();
    assert!(reopened.get(&thread).is_some(), "the entry survives");
    assert_eq!(reopened.get(&thread).unwrap().session, minted);
    assert!(
        !reopened.has_transcript(&thread),
        "a failed first turn must not look like a conversation"
    );

    // And once a turn completes it is a conversation, across a restart.
    let mut registry = Registry::open(&path).unwrap();
    registry.record_turn(&thread).unwrap();
    assert!(registry.has_transcript(&thread));
    assert!(Registry::open(&path).unwrap().has_transcript(&thread));
}
