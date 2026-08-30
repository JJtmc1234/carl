use super::*;
use crate::claude::Say;

fn home() -> tempfile::TempDir {
    tempfile::tempdir().expect("a temp home")
}

/// The whole point. A turn that produced no words still leaves a trace of what happened.
#[test]
fn a_turn_that_only_used_tools_is_still_visible() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.asked("read the mail");
    w.saw(Say::Doing {
        tool: "Read",
        detail: "memory/summary.md",
    });
    w.saw(Say::Doing {
        tool: "search_threads",
        detail: "is:unread",
    });
    w.answered("", false);

    let got = read(d.path(), None, 50).expect("read");
    assert_eq!(got.len(), 4, "{got:?}");
    assert!(matches!(got[1].note, Note::Doing { ref tool, .. } if tool == "Read"));
}

/// Words are deliberately not kept. A second copy of an answer is a second thing to disagree
/// with the transcript.
#[test]
fn the_answer_itself_is_not_copied_into_the_notes() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.saw(Say::Words("the secret is hunter2"));

    assert!(read(d.path(), None, 50).expect("read").is_empty());
}

/// Thinking deltas arrive every few tokens. One note each would be thousands of rows saying the
/// same thing slightly larger, and the file would be useless at exactly the moment it is needed.
#[test]
fn reasoning_is_recorded_as_it_grows_rather_than_on_every_delta() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    for tokens in 1..=500 {
        w.saw(Say::Thinking {
            text: "",
            tokens: Some(tokens),
        });
    }

    let got = read(d.path(), None, 1000).expect("read");
    assert!(
        got.len() <= 6,
        "500 deltas became {} rows, which nobody can read",
        got.len()
    );
    assert!(!got.is_empty(), "coalescing dropped the reasoning entirely");
}

/// The day the CLI stops redacting, this file is the reason nothing has to change.
#[test]
fn real_reasoning_text_is_never_dropped_by_the_coalescing() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    for _ in 0..5 {
        w.saw(Say::Thinking {
            text: "checking whether the belt is actually the constraint",
            tokens: Some(1),
        });
    }

    let got = read(d.path(), None, 50).expect("read");
    assert_eq!(
        got.len(),
        5,
        "reasoning with words in it was coalesced away"
    );
}

/// A refusal is addressed to the person who can widen the allow list, so it has to survive.
#[test]
fn a_refused_tool_call_is_kept_with_its_reason() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.saw(Say::Refused {
        tool: "Bash",
        why: "not on the allow list",
    });

    let got = read(d.path(), None, 50).expect("read");
    match &got[0].note {
        Note::Refused { tool, why } => {
            assert_eq!(tool, "Bash");
            assert!(why.contains("allow list"), "{why}");
        }
        other => panic!("a refusal was recorded as {other:?}"),
    }
}

/// A whole file written through a tool call must not become the whole log.
#[test]
fn a_huge_tool_input_is_cut_down() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    w.saw(Say::Doing {
        tool: "Write",
        detail: &"x".repeat(50_000),
    });

    let got = read(d.path(), None, 50).expect("read");
    match &got[0].note {
        Note::Doing { detail, .. } => assert!(
            detail.chars().count() <= MOST_DETAIL + 3,
            "{}",
            detail.len()
        ),
        other => panic!("{other:?}"),
    }
}

/// One note is one row, so a multi line tool input must not look like several things happening.
#[test]
fn no_note_spills_onto_a_second_row() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    w.saw(Say::Doing {
        tool: "Write",
        detail: "line one\nline two\nline three",
    });

    let now = crate::army::event::now();
    for line in read(d.path(), None, 50).expect("read") {
        assert!(!line_of(&line, now).contains('\n'), "{line:?}");
    }
}

/// Asking about one agent must not show another's, or the answer to "what is Miles doing" is
/// everybody at once.
#[test]
fn one_agent_can_be_watched_without_the_rest() {
    let d = home();
    for who in ["miles", "nora", "mason"] {
        let mut w = Watching::of(d.path(), who);
        w.asked("something");
    }

    let got = read(d.path(), Some("nora"), 50).expect("read");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].agent, "nora");
}

/// A home nobody has worked in yet answers rather than failing.
#[test]
fn an_empty_home_is_readable() {
    let d = home();
    assert!(read(d.path(), None, 20).expect("read").is_empty());
    assert_eq!(since(d.path(), 0, None).expect("since").0.len(), 0);
}

/// A torn line must not take the rest of the file with it.
#[test]
fn a_torn_line_does_not_stop_the_rest_being_read() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.asked("first");
    w.asked("second");

    let p = path(d.path());
    let good = std::fs::read_to_string(&p).expect("read");
    std::fs::write(&p, format!("{{not json\n{good}")).expect("write");

    assert_eq!(read(d.path(), None, 50).expect("read").len(), 2);
}

/// Following the file gives back what arrived and nothing twice.
#[test]
fn following_picks_up_only_what_is_new() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.asked("first");

    let (first, at) = since(d.path(), 0, None).expect("since");
    assert_eq!(first.len(), 1);

    let (nothing, at) = since(d.path(), at, None).expect("since");
    assert!(nothing.is_empty(), "a line was handed over twice");

    w.asked("second");
    let (more, _) = since(d.path(), at, None).expect("since");
    assert_eq!(more.len(), 1, "the new line was missed");
}

/// The file is trimmed under us while something is following it. Reading from a stale offset
/// would then hand back the middle of a line.
#[test]
fn a_trimmed_file_is_followed_from_the_start_again() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    for _ in 0..20 {
        w.asked("something");
    }
    let (_, at) = since(d.path(), 0, None).expect("since");

    let p = path(d.path());
    let text = std::fs::read_to_string(&p).expect("read");
    let half: String = text.lines().take(3).map(|l| format!("{l}\n")).collect();
    std::fs::write(&p, half).expect("write");

    let (got, _) = since(d.path(), at, None).expect("since");
    assert_eq!(got.len(), 3, "a shorter file was read from the old offset");
}

/// Every kind renders as words rather than as its own struct.
#[test]
fn no_row_leaks_a_debug_struct() {
    let d = home();
    let mut w = Watching::of(d.path(), "miles");
    w.asked("q");
    // The sizeless one first, so both are written: the second is only recorded because the
    // count has grown, and a count that was never given cannot grow.
    w.saw(Say::Thinking {
        text: "",
        tokens: None,
    });
    w.saw(Say::Thinking {
        text: "",
        tokens: Some(400),
    });
    w.saw(Say::Doing {
        tool: "Read",
        detail: "",
    });
    w.saw(Say::Refused {
        tool: "Bash",
        why: "no",
    });
    w.answered("hello", true);

    let now = crate::army::event::now();
    let rows: Vec<String> = read(d.path(), None, 50)
        .expect("read")
        .iter()
        .map(|l| line_of(l, now))
        .collect();
    assert_eq!(rows.len(), 6);
    for row in &rows {
        assert!(!row.contains(" { "), "a struct leaked: {row}");
    }
    assert!(rows.last().expect("a row").contains("ran out of time"));
}

/// The cap has to actually hold, or a long night fills the disk.
#[test]
fn the_file_stops_growing_at_its_cap() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    // Enough tool calls at the detail cap to go past two megabytes several times over.
    let detail = "x".repeat(MOST_DETAIL);
    for _ in 0..20_000 {
        w.saw(Say::Doing {
            tool: "Write",
            detail: &detail,
        });
    }

    let len = std::fs::metadata(path(d.path())).expect("metadata").len();
    assert!(len <= MOST_BYTES, "the file grew to {len} bytes");
    assert!(
        !read(d.path(), None, 10).expect("read").is_empty(),
        "trimming took everything"
    );
}

/// A CLI that gives no sizes must still leave a trace that reasoning happened, or the one thing
/// this file exists to show disappears the day the shape of the stream changes.
#[test]
fn reasoning_with_no_size_at_all_is_still_recorded_once() {
    let d = home();
    let mut w = Watching::of(d.path(), "nora");
    for _ in 0..50 {
        w.saw(Say::Thinking {
            text: "",
            tokens: None,
        });
    }

    let got = read(d.path(), None, 50).expect("read");
    assert_eq!(got.len(), 1, "{got:?}");
}
