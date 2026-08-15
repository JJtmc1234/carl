//! Diffs, against a real git repository built for the test.

use super::*;

/// A real repository with one commit, which is the fixture every git test here needs.
fn repository() -> (PathBuf, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .expect("git should run");
        assert!(out.status.success(), "git {args:?}: {:?}", out);
    };

    git(&["init", "--quiet", "-b", "main"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    std::fs::write(root.join("thing.txt"), "one\ntwo\nthree\n").unwrap();
    git(&["add", "thing.txt"]);
    git(&["commit", "--quiet", "-m", "first"]);

    (root, d)
}

/// The guard that stops a future change implementing a diff by checking something out.
#[test]
fn a_git_command_that_writes_is_refused() {
    let (root, _d) = repository();
    for dangerous in ["checkout", "stash", "reset", "commit", "clean", "restore"] {
        let err = run(&root, &[dangerous, "--hard"]).unwrap_err().to_string();
        assert!(
            err.contains("read only"),
            "{dangerous} should be refused, got {err}"
        );
    }
    assert!(run(&root, &[]).is_err(), "no subcommand at all is refused");
}

#[test]
fn a_read_only_command_is_allowed() {
    let (root, _d) = repository();
    let out = run(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
    assert_eq!(out.trim(), "main");
}

#[test]
fn a_clean_file_shows_no_difference_against_head() {
    let (root, _d) = repository();
    let diff = against_head(&root, Path::new("thing.txt")).unwrap();
    assert!(diff.is_empty(), "nothing changed, got {diff:?}");
}

#[test]
fn an_edited_file_shows_its_difference_against_head() {
    let (root, _d) = repository();
    std::fs::write(root.join("thing.txt"), "one\nCHANGED\nthree\n").unwrap();

    let diff = against_head(&root, Path::new("thing.txt")).unwrap();
    assert!(diff.contains("-two"), "{diff}");
    assert!(diff.contains("+CHANGED"), "{diff}");
}

#[test]
fn a_working_tree_lists_what_changed_in_it() {
    let (root, _d) = repository();
    std::fs::write(root.join("thing.txt"), "edited\n").unwrap();
    std::fs::write(root.join("new.txt"), "brand new\n").unwrap();

    let changes = worktree_changes(&root).unwrap();
    let paths: Vec<String> = changes
        .iter()
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect();
    assert!(paths.contains(&"thing.txt".to_string()), "{paths:?}");
    assert!(paths.contains(&"new.txt".to_string()), "{paths:?}");

    let untracked = changes
        .iter()
        .find(|c| c.path.ends_with("new.txt"))
        .unwrap();
    assert!(untracked.is_untracked());
}

#[test]
fn a_clean_tree_summarises_as_clean() {
    let (root, _d) = repository();
    assert_eq!(summarise(&root).unwrap(), "no local changes");
}

#[test]
fn a_dirty_tree_summarises_what_kind_of_dirty() {
    let (root, _d) = repository();
    std::fs::write(root.join("thing.txt"), "edited\n").unwrap();
    assert_eq!(summarise(&root).unwrap(), "1 changed");

    std::fs::write(root.join("new.txt"), "new\n").unwrap();
    assert_eq!(summarise(&root).unwrap(), "1 changed, 1 untracked");
}

#[test]
fn a_repository_is_found_from_a_file_inside_it() {
    let (root, _d) = repository();
    let found = repository_of(&root.join("thing.txt")).expect("inside a repository");
    assert_eq!(found, root);

    let from_dir = repository_of(&root).expect("the root is inside itself");
    assert_eq!(from_dir, root);
}

#[test]
fn somewhere_that_is_not_a_repository_has_none() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(repository_of(d.path()), None);
}

#[test]
fn asking_about_a_path_outside_any_repository_is_an_error_rather_than_a_panic() {
    let d = tempfile::tempdir().unwrap();
    assert!(worktree_changes(d.path()).is_err());
}

#[test]
fn identical_text_has_no_difference() {
    assert_eq!(simple_diff("same\n", "same\n"), "");
    assert_eq!(buffer_vs_disk("a\nb\n", "a\nb\n"), "");
}

#[test]
fn a_changed_line_shows_as_removed_and_added() {
    let out = simple_diff("one\ntwo\nthree\n", "one\nCHANGED\nthree\n");
    assert!(out.contains("-two"), "{out}");
    assert!(out.contains("+CHANGED"), "{out}");
    assert!(!out.contains("one"), "the common start is trimmed: {out}");
    assert!(out.contains("line 2"), "and it says where: {out}");
}

#[test]
fn an_added_line_shows_only_as_added() {
    let out = simple_diff("one\ntwo\n", "one\ntwo\nthree\n");
    assert!(out.contains("+three"), "{out}");
    assert!(!out.contains('-'), "nothing was removed: {out}");
}

#[test]
fn a_removed_line_shows_only_as_removed() {
    let out = simple_diff("one\ntwo\nthree\n", "one\nthree\n");
    assert!(out.contains("-two"), "{out}");
    assert!(!out.contains('+'), "nothing was added: {out}");
}

#[test]
fn text_that_became_empty_is_all_removed() {
    let out = simple_diff("one\ntwo\n", "");
    assert!(out.contains("-one"), "{out}");
    assert!(out.contains("-two"), "{out}");
}

#[test]
fn text_that_came_from_nothing_is_all_added() {
    let out = simple_diff("", "one\n");
    assert!(out.contains("+one"), "{out}");
}

/// The head and tail trimming must not count the same line twice, which is what would happen
/// on a short file made entirely of repeated lines.
#[test]
fn repeated_lines_do_not_confuse_the_trimming() {
    let out = simple_diff("a\na\na\n", "a\na\n");
    assert_eq!(out.matches("-a").count(), 1, "exactly one line went: {out}");
    assert!(!out.contains("+a"), "nothing was added: {out}");
}

/// The unsaved buffer case, which git cannot see because the text is not on disk.
#[test]
fn an_unsaved_buffer_can_be_compared_with_the_file_it_came_from() {
    let (root, _d) = repository();
    let on_disk = std::fs::read_to_string(root.join("thing.txt")).unwrap();
    let out = buffer_vs_disk(&on_disk, "one\ntwo\nthree\nfour\n");
    assert!(out.contains("+four"), "{out}");
}
