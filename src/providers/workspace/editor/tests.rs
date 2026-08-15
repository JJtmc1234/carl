//! The editor, against real files in a temporary directory.

use super::*;

fn file_with(text: &str) -> (PathBuf, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("thing.rs");
    std::fs::write(&path, text).unwrap();
    (path, d)
}

#[test]
fn a_file_opens_with_its_contents_and_its_extension() {
    let (path, _d) = file_with("fn main() {}\n");
    let open = open(&path, Mode::ReadWrite).unwrap();

    assert_eq!(open.text(), "fn main() {}\n");
    assert_eq!(open.extension(), Some("rs"));
    assert!(!open.is_read_only());
    assert!(!open.changed_on_disk());
}

#[test]
fn saving_writes_the_new_text_and_leaves_no_staging_file() {
    let (path, d) = file_with("before\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    open.save("after\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
    assert_eq!(open.text(), "after\n");

    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains("writing"))
        .collect();
    assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
}

#[test]
fn saving_twice_in_a_row_works_because_the_fingerprint_is_updated() {
    let (path, _d) = file_with("one\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    open.save("two\n").unwrap();
    open.save("three\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
}

/// The reason this module exists. An agent editing the same file must not lose its work to a
/// save from the panel.
#[test]
fn a_save_is_refused_when_the_file_changed_underneath() {
    let (path, _d) = file_with("original\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    std::fs::write(&path, "somebody else got here first\n").unwrap();
    assert!(open.changed_on_disk());

    let err = open.save("my version\n").unwrap_err().to_string();
    assert!(err.contains("changed on disk"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "somebody else got here first\n",
        "the other edit survived"
    );
}

/// The escape hatch, which is a decision rather than a race.
#[test]
fn reloading_lets_a_refused_save_go_through_afterwards() {
    let (path, _d) = file_with("original\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    std::fs::write(&path, "theirs\n").unwrap();
    assert!(open.save("mine\n").is_err());

    open.reload().unwrap();
    assert_eq!(open.text(), "theirs\n");
    assert!(!open.changed_on_disk());
    open.save("mine\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");
}

/// The case length and modified time alone would miss.
#[test]
fn a_same_length_edit_in_the_same_second_is_still_noticed() {
    let (path, _d) = file_with("aaaa\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    std::fs::write(&path, "bbbb\n").unwrap();
    assert!(open.changed_on_disk(), "the hash is what catches this");
    assert!(open.save("cccc\n").is_err());
}

#[test]
fn a_file_deleted_underneath_counts_as_changed() {
    let (path, _d) = file_with("here\n");
    let mut open = open(&path, Mode::ReadWrite).unwrap();

    std::fs::remove_file(&path).unwrap();
    assert!(open.changed_on_disk());
    assert!(
        open.save("back again\n").is_err(),
        "a save must not resurrect it"
    );
}

#[test]
fn a_read_only_file_refuses_to_save_rather_than_doing_nothing() {
    let (path, _d) = file_with("look but do not touch\n");
    let mut open = open(&path, Mode::ReadOnly).unwrap();

    assert!(open.is_read_only());
    let err = open.save("touched\n").unwrap_err().to_string();
    assert!(err.contains("read only"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "look but do not touch\n"
    );
}

#[test]
fn a_directory_is_not_a_file_to_open() {
    let d = tempfile::tempdir().unwrap();
    let err = open(d.path(), Mode::ReadWrite).unwrap_err().to_string();
    assert!(err.contains("is a directory"), "{err}");
}

#[test]
fn a_file_that_is_not_there_says_so() {
    let d = tempfile::tempdir().unwrap();
    let err = open(d.path().join("nope.txt"), Mode::ReadWrite)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot open"), "{err}");
}

/// A text box handed arbitrary bytes shows nonsense, and saving turns the file into something
/// the program that owns it can no longer read.
#[test]
fn a_binary_file_is_refused_rather_than_mangled() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("thing.bin");
    std::fs::write(&path, [0xff, 0xfe, 0x00, 0x01, 0x80]).unwrap();

    let err = open(&path, Mode::ReadWrite).unwrap_err().to_string();
    assert!(err.contains("is not text"), "{err}");
}

#[test]
fn a_file_too_large_to_hold_is_refused_with_its_size() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("huge.txt");
    let big = vec![b'a'; (MAX_BYTES + 1) as usize];
    std::fs::write(&path, big).unwrap();

    let err = open(&path, Mode::ReadWrite).unwrap_err().to_string();
    assert!(err.contains("larger than"), "{err}");
}

/// Two paths to the same file have to be one file, or the stale check can be walked around.
#[test]
fn a_path_through_a_symlink_resolves_to_the_real_file() {
    let d = tempfile::tempdir().unwrap();
    let real = d.path().join("real.txt");
    std::fs::write(&real, "contents\n").unwrap();

    let link = d.path().join("link.txt");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let open = open(&link, Mode::ReadWrite).unwrap();
    assert_eq!(open.path(), real.canonicalize().unwrap());
}

#[test]
fn a_path_with_dot_dot_in_it_resolves_rather_than_escaping_quietly() {
    let d = tempfile::tempdir().unwrap();
    let inner = d.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    let target = d.path().join("outer.txt");
    std::fs::write(&target, "x\n").unwrap();

    let sneaky = inner.join("..").join("outer.txt");
    let open = open(&sneaky, Mode::ReadWrite).unwrap();
    assert_eq!(
        open.path(),
        target.canonicalize().unwrap(),
        "the caller can see exactly which file was opened"
    );
}

#[test]
fn a_file_with_no_extension_still_opens_and_still_saves() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("Makefile");
    std::fs::write(&path, "all:\n").unwrap();

    let mut open = open(&path, Mode::ReadWrite).unwrap();
    assert_eq!(open.extension(), None);
    open.save("all:\n\techo hi\n").unwrap();
    assert!(std::fs::read_to_string(&path).unwrap().contains("echo hi"));
}

#[test]
fn an_empty_file_is_a_file() {
    let (path, _d) = file_with("");
    let mut open = open(&path, Mode::ReadWrite).unwrap();
    assert_eq!(open.text(), "");
    open.save("now it has something\n").unwrap();
}
