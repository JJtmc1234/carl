use super::*;
use crate::army::org;
use crate::army::personnel::{Corrector, memory};

fn folder() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().expect("a temp dir");
    let f = d.path().join("nora");
    std::fs::create_dir_all(&f).expect("the folder");
    (d, f)
}

fn seeded() -> (tempfile::TempDir, std::path::PathBuf) {
    let (d, f) = folder();
    memory::seed(&f, org::require("nora").expect("nora")).expect("seed");
    (d, f)
}

/// A freshly seeded agent is complete, or seeding is not doing its job.
#[test]
fn a_seeded_folder_is_healthy() {
    let (_d, f) = seeded();
    let h = of(&f);
    assert_eq!(h.memory, Memory::Fine);
    assert!(!h.memory.is_a_problem());
    assert_eq!(h.rules, Some(0));
    assert_eq!(h.watching, Some(0));
    assert!(h.summary_bytes > 0);
    assert!(!h.legacy_rules);
}

/// The failure this module exists for. A process can be up and the folder can be gone.
#[test]
fn no_folder_at_all_is_reported_rather_than_looking_idle() {
    let (_d, f) = folder();
    let h = of(&f);
    assert_eq!(h.memory, Memory::NoFolder);
    assert!(h.memory.is_a_problem());
    assert!(
        h.memory.why().contains("starts from nothing"),
        "{}",
        h.memory.why()
    );
}

#[test]
fn a_folder_with_no_summary_is_reported() {
    let (_d, f) = seeded();
    std::fs::remove_file(memory::summary_path(&f)).expect("remove");
    assert_eq!(of(&f).memory, Memory::NoSummary);
}

/// Made before the layout existed. Fixable, and the message says how.
#[test]
fn a_folder_with_no_learned_file_is_unmigrated() {
    let (_d, f) = seeded();
    std::fs::remove_file(memory::learned_path(&f)).expect("remove");
    let h = of(&f);
    assert!(matches!(h.memory, Memory::Unmigrated(_)), "{:?}", h.memory);
    assert!(
        h.memory.why().contains("carl army migrate"),
        "{}",
        h.memory.why()
    );
}

/// The quiet one. The old file still holds the only rules there are and nothing reads it, so
/// the agent looks fine and has silently lost its standing decisions.
#[test]
fn a_legacy_file_holding_the_only_rules_is_unmigrated() {
    let (_d, f) = seeded();
    std::fs::write(
        memory::dir(&f).join(memory::LEGACY_RULES),
        "- Miss Candi is school and always important\n",
    )
    .expect("write");

    let h = of(&f);
    assert!(matches!(h.memory, Memory::Unmigrated(_)), "{:?}", h.memory);
    assert!(h.legacy_rules);
}

/// Once migration has run the legacy file is still on disk and is no longer a problem, because
/// what it held is now somewhere that is read.
#[test]
fn a_legacy_file_is_fine_once_its_rules_have_been_migrated() {
    let (_d, f) = seeded();
    std::fs::write(
        memory::dir(&f).join(memory::LEGACY_RULES),
        "- Miss Candi is school and always important\n",
    )
    .expect("write");
    memory::migrate(&f).expect("migrate");

    let h = of(&f);
    assert_eq!(h.memory, Memory::Fine, "{:?}", h.memory);
    assert!(h.legacy_rules, "the old file should still be there");
    assert_eq!(h.rules, Some(1));
}

/// A file that is there and is not what it says it is. Reported as its own state, because
/// "unreadable" and "empty" are different problems and only one is fixed by migrating.
#[test]
fn an_unreadable_learned_file_is_malformed_rather_than_missing() {
    let (_d, f) = seeded();
    // A directory where a file belongs. Reading it fails, which is the honest case to cover.
    std::fs::remove_file(memory::learned_path(&f)).expect("remove");
    std::fs::create_dir(memory::learned_path(&f)).expect("mkdir");

    let h = of(&f);
    assert!(matches!(h.memory, Memory::Malformed(_)), "{:?}", h.memory);
    assert_eq!(
        h.rules, None,
        "a count must not be invented for a broken file"
    );
}

/// Counts come from the file, not from anywhere else.
#[test]
fn the_counts_are_read_from_the_file() {
    let (_d, f) = seeded();
    let mut learned = crate::army::personnel::Learned::default();
    learned.corrected(Corrector::Jj, "Vendor X invoices from billing@x.example");
    learned.observe("ASUS mail is promotional");
    learned.save(&memory::learned_path(&f)).expect("save");

    let h = of(&f);
    assert_eq!(h.rules, Some(1));
    assert_eq!(h.watching, Some(1));
}

/// Reporting must never repair. A report that fixed what it described would be describing its
/// own side effect.
#[test]
fn reading_health_never_writes_anything() {
    let (_d, f) = seeded();
    std::fs::remove_file(memory::learned_path(&f)).expect("remove");

    let _ = of(&f);
    let _ = of(&f);
    assert!(
        !memory::learned_path(&f).exists(),
        "reporting created the file it was reporting as missing"
    );
}
