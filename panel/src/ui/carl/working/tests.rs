use super::*;

fn call(tool: &str, detail: &str) -> ToolCall {
    ToolCall {
        tool: tool.into(),
        detail: detail.into(),
    }
}

#[test]
fn a_tool_with_no_detail_is_just_its_name() {
    assert_eq!(line_for(&call("Read", "")), "  > Read");
}

#[test]
fn a_tool_with_a_detail_carries_it() {
    assert_eq!(
        line_for(&call("Bash", "ls -la /tmp")),
        "  > Bash ls -la /tmp"
    );
}

/// A multi line command must not take the row count with it. One call is one line.
#[test]
fn a_multi_line_command_is_flattened_to_one_line() {
    let out = line_for(&call("Bash", "cat <<EOF\nhello\nthere\nEOF"));
    assert!(!out.contains('\n'), "{out:?}");
    assert!(out.contains("cat <<EOF hello there EOF"), "{out:?}");
}

#[test]
fn a_long_detail_is_cut_and_marked() {
    let out = line_for(&call("Grep", &"x".repeat(400)));
    assert!(out.ends_with("..."), "{out:?}");
    assert!(out.chars().count() < 90, "{} chars", out.chars().count());
}

/// Cutting happens on character boundaries. A byte slice through a multi byte character
/// panics, and a path with an accent in it is not exotic.
#[test]
fn cutting_a_detail_never_splits_a_character() {
    let out = line_for(&call("Read", &"é".repeat(400)));
    assert!(out.ends_with("..."), "{out:?}");
}

/// While it is still arriving the heading has to move, because a still indicator cannot be
/// told from a stuck one. That was the whole complaint about the old placeholder.
#[test]
fn the_heading_follows_the_tail_while_it_is_arriving() {
    let short = heading_for("just started", true);
    let long = heading_for(&format!("just started {}", "and then ".repeat(40)), true);
    assert!(short.starts_with("THINKING"), "{short:?}");
    assert!(long.starts_with("THINKING"), "{long:?}");
    assert_ne!(short, long, "the heading has to change as text arrives");
    assert!(long.contains("..."), "{long:?}");
}

/// Once it is finished there is nothing to watch, so the heading says how much there is to
/// read instead of showing a fragment that is no longer moving.
#[test]
fn a_finished_heading_gives_the_size_rather_than_a_fragment() {
    let out = heading_for("some reasoning", false);
    assert_eq!(out, "REASONING, 14 characters");
}

#[test]
fn a_finished_heading_never_pretends_it_is_still_thinking() {
    assert!(!heading_for("done", false).contains("THINKING"));
}
