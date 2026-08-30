use super::*;

/// The whole security argument in one test. If this ever passes for `exec`, every tool list in
/// the codebase has become decoration.
#[test]
fn the_dispatchers_that_would_be_a_shell_are_refused() {
    for dangerous in [
        "exec",
        "execr",
        "exit",
        "killactive",
        "closewindow",
        "keyword",
        "dpms",
        "movecursor",
        "forcerendererreload",
    ] {
        assert!(
            !ALLOWED.iter().any(|(name, _)| *name == dangerous),
            "{dangerous} is on the allow list"
        );
    }
}

/// An unknown dispatcher must be refused without being run, and the refusal has to say what is
/// allowed, because the agent reading it is the one who has to pick again.
#[test]
fn an_unknown_dispatcher_is_refused_by_name_and_says_what_is_allowed() {
    let err = dispatch("exec", "rm -rf /").expect_err("exec must never run");
    let said = err.to_string();
    assert!(said.contains("exec"), "{said}");
    assert!(
        said.contains("workspace"),
        "the refusal lists nothing: {said}"
    );
}

/// Every allowed dispatcher only ever moves a window or changes a workspace. This is the list
/// somebody will add to in a hurry, and the comment explaining the rule is easy to skip.
#[test]
fn nothing_on_the_allow_list_can_run_a_program_or_end_the_session() {
    for (name, what) in ALLOWED {
        assert!(
            !name.contains("exec") && !name.contains("exit") && !name.contains("kill"),
            "{name} looks like it does more than move windows"
        );
        assert!(!what.is_empty(), "{name} has no description");
    }
}

/// Refusing an ambiguous name rather than picking one. Two Chrome windows and "focus chrome" is
/// a question, and answering it silently is how the wrong window gets moved and reported fine.
#[test]
fn an_ambiguous_window_name_is_refused_rather_than_guessed() {
    let windows = vec![
        client("google-chrome", "Zoom Meeting"),
        client("google-chrome", "GitHub"),
        client("code", "MEMORY.md"),
    ];

    let err = read::only_match(&windows, "chrome").expect_err("two matches must refuse");
    assert!(err.to_string().contains('2'), "{err}");

    let one = read::only_match(&windows, "code").expect("one match");
    assert_eq!(one.class, "code");
}

#[test]
fn a_window_nobody_has_open_is_refused_with_where_to_look() {
    let windows = vec![client("code", "MEMORY.md")];
    let err = read::only_match(&windows, "firefox").expect_err("no match");
    assert!(err.to_string().contains("carl hypr windows"), "{err}");
}

/// The Command Panel reports an empty class, which is exactly the window an agent is most
/// likely to be asked about. Falling back to the title is what makes it addressable at all.
#[test]
fn a_window_with_no_class_is_still_named_and_findable() {
    let windows = vec![client("", "AOS Command Panel")];
    assert_eq!(windows[0].name(), "AOS Command Panel");
    assert!(read::only_match(&windows, "command panel").is_ok());
}

/// Matching is on what a person would type, not on exact case.
#[test]
fn matching_a_window_is_not_case_sensitive() {
    let windows = vec![client("Google-Chrome", "GitHub")];
    assert!(read::only_match(&windows, "google-chrome").is_ok());
    assert!(read::only_match(&windows, "GITHUB").is_ok());
}

/// Off a Hyprland session every call has to say so rather than failing as a missing binary.
#[test]
fn every_call_refuses_clearly_when_hyprland_is_not_running() {
    if running() {
        return;
    }
    let err = clients().expect_err("not on Hyprland");
    assert!(err.to_string().contains("Hyprland is not running"), "{err}");
}

fn client(class: &str, title: &str) -> Client {
    Client {
        address: "0x1".into(),
        class: class.into(),
        title: title.into(),
        workspace: Workspace::default(),
        floating: false,
        fullscreen: 0,
        pid: 1,
    }
}
