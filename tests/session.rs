//! A held open conversation, driven against a stand in for `claude`.
//!
//! The session is the part with the most ways to go quietly wrong. It reads a stream, hands
//! back a turn, and has to survive somebody walking away from an answer that is still being
//! written. None of that is provable against the real binary in a test, because a model is
//! slow, costs money, and never produces the same thing twice.
//!
//! So the stand in speaks the same protocol and nothing else. It reads one JSON message per
//! line and answers in the shape Claude Code answers in, which is exactly the contract the
//! session depends on.

use std::io::Write;
use std::path::PathBuf;

use carl::SessionId;
use carl::claude::{Flow, Runner};

/// Writes a script that answers like Claude Code does, one word at a time.
///
/// `delay` is seconds between the words, which is how an answer that is still being written
/// gets simulated without waiting on a real model.
fn fake_claude(dir: &std::path::Path, delay: &str, words: &[&str]) -> PathBuf {
    let path = dir.join("fake-claude");
    let deltas: String = words
        .iter()
        .map(|w| {
            // Sleeps before the word, not after, so a delay of 0.4 means the first word
            // genuinely arrives at 0.4 seconds. Printing first and sleeping after makes every
            // answer start instantly, which quietly defeats any test about waiting.
            format!(
                "  sleep {delay}\n  printf '{{\"type\":\"stream_event\",\"event\":{{\"type\":\"content_block_delta\",\"delta\":{{\"type\":\"text_delta\",\"text\":\"%s%s \"}}}}}}\\n' '{w}' \"$turn\"\n"
            )
        })
        .collect();

    // Every turn is numbered, and that is not decoration. Without it each answer is identical,
    // so an answer left over from an abandoned turn reads exactly like a fresh one and any
    // test about clearing them passes whether the clearing happens or not. That was the first
    // version of this file and it proved nothing.
    let script = format!(
        "#!/bin/bash\n\
         turn=0\n\
         while IFS= read -r line; do\n\
         \x20 turn=$((turn+1))\n\
         {deltas}\
           printf '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"done %s\",\"session_id\":\"fake\"}}\\n' \"$turn\"\n\
         done\n"
    );

    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(script.as_bytes()).unwrap();
    drop(f);

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn session(dir: &std::path::Path, delay: &str, words: &[&str]) -> carl::claude::Session {
    let program = fake_claude(dir, delay, words);
    let runner = Runner::at(program);

    // Retried because of a race that belongs to the test and not to Carl. These tests run in
    // parallel threads of one process, and between a fork and its exec the child briefly
    // holds every open file descriptor, including another thread's handle on the script it is
    // still writing. Linux refuses to exec a file anybody has open for writing, so the answer
    // is `Text file busy` and the answer is to try again.
    let mut last = None;
    for _ in 0..20 {
        match runner.open_session(&SessionId::fresh().unwrap(), dir, "you are carl", false) {
            Ok(s) => return s,
            Err(e) => {
                last = Some(e);
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
    panic!("the stand in never started: {}", last.unwrap());
}

#[test]
fn one_question_gets_one_answer() {
    let d = tempfile::tempdir().unwrap();
    let mut s = session(d.path(), "0", &["hello", "there"]);

    let mut heard = String::new();
    let answer = s
        .ask(
            "anything",
            &mut |t| {
                if let Some(words) = t.words() {
                    heard.push_str(words);
                }
                Flow::Continue
            },
            &mut || Flow::Continue,
        )
        .unwrap();

    assert_eq!(heard.trim(), "hello1 there1");
    assert_eq!(answer.text, "done 1");
    assert!(!answer.interrupted);
}

/// The whole reason a session exists. Two questions, one process, and the second must not be
/// answered with anything left over from the first.
#[test]
fn a_second_question_gets_its_own_answer() {
    let d = tempfile::tempdir().unwrap();
    let mut s = session(d.path(), "0", &["one"]);

    for n in 1..=3 {
        let mut heard = String::new();
        let answer = s
            .ask(
                "again",
                &mut |t| {
                    if let Some(words) = t.words() {
                    heard.push_str(words);
                }
                    Flow::Continue
                },
                &mut || Flow::Continue,
            )
            .unwrap();
        assert_eq!(
            heard.trim(),
            format!("one{n}"),
            "turn {n} must get turn {n}'s words"
        );
        assert_eq!(answer.text, format!("done {n}"));
    }
}

/// Interrupting Carl while he thinks, which is the thing that did not work. Nothing has been
/// said yet, so there is no `on_text` to return Stop from, and without a check while waiting
/// the question you abandoned still gets answered at you.
#[test]
fn a_turn_can_be_given_up_on_before_its_first_word() {
    let d = tempfile::tempdir().unwrap();
    // Slow enough that the answer has definitely not started when the caller gives up.
    let mut s = session(d.path(), "0.4", &["much", "too", "late"]);

    let mut ticks = 0;
    let answer = s
        .ask("never mind", &mut |_| Flow::Continue, &mut || {
            ticks += 1;
            // Two ticks is about 200ms, well before the first word at 400ms.
            if ticks >= 2 {
                Flow::Stop
            } else {
                Flow::Continue
            }
        })
        .unwrap();

    assert!(answer.interrupted, "it should have been given up on");
    assert!(
        answer.text.trim().is_empty(),
        "nothing was said, so nothing should come back: {:?}",
        answer.text
    );
}

/// The trap underneath giving up. The model does not stop because the listener walked away,
/// so the rest of that answer is still coming. Without clearing it, the next question is
/// handed the tail of the last one.
#[test]
fn the_next_question_is_not_answered_with_the_abandoned_one() {
    let d = tempfile::tempdir().unwrap();
    let mut s = session(d.path(), "0.2", &["stale", "stale", "stale"]);

    let mut ticks = 0;
    let first = s
        .ask("forget this", &mut |_| Flow::Continue, &mut || {
            ticks += 1;
            if ticks >= 1 {
                Flow::Stop
            } else {
                Flow::Continue
            }
        })
        .unwrap();
    assert!(first.interrupted);

    let mut heard = String::new();
    let second = s
        .ask(
            "now this",
            &mut |t| {
                if let Some(words) = t.words() {
                    heard.push_str(words);
                }
                Flow::Continue
            },
            &mut || Flow::Continue,
        )
        .unwrap();

    // The numbers are the whole point. Turn one's leftovers say stale1 and turn two says
    // stale2, so hearing any 1 here means the abandoned answer was handed over as this one.
    assert!(
        !heard.contains('1'),
        "the abandoned answer leaked into this one: {heard:?}"
    );
    assert_eq!(
        heard.split_whitespace().count(),
        3,
        "and this answer should be whole: {heard:?}"
    );
    assert_eq!(second.text, "done 2");
    assert!(!second.interrupted);
}

/// Stopping once the words have started is the older behaviour, and it must still hold.
#[test]
fn a_turn_can_still_be_cut_off_once_it_has_started() {
    let d = tempfile::tempdir().unwrap();
    let mut s = session(d.path(), "0.1", &["one", "two", "three", "four"]);

    let mut heard = String::new();
    let answer = s
        .ask(
            "go on",
            &mut |t| {
                if let Some(words) = t.words() {
                    heard.push_str(words);
                }
                if heard.contains("two") {
                    Flow::Stop
                } else {
                    Flow::Continue
                }
            },
            &mut || Flow::Continue,
        )
        .unwrap();

    assert!(answer.interrupted);
    assert!(answer.text.contains("one1"), "what was said is kept");
    assert!(!answer.text.contains("four1"), "and the rest is not");
}

/// An empty question reaches the CLI as no question at all, and it complains in a way that
/// says nothing about where the empty question came from.
#[test]
fn an_empty_question_never_reaches_the_process() {
    let d = tempfile::tempdir().unwrap();
    let mut s = session(d.path(), "0", &["unused"]);

    for empty in ["", "   ", "\n\t "] {
        let err = s
            .ask(empty, &mut |_| Flow::Continue, &mut || Flow::Continue)
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty question"), "{err}");
    }
}
