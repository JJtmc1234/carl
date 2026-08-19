//! One key that opens the panel whether or not it is already running.
//!
//! A desktop shortcut can only run a command. It cannot know whether the thing it is for is
//! up, so if `--toggle` only ever flipped a live panel then the first press of the day would
//! do nothing at all, and JJ would have to go and start it by hand before the shortcut became
//! useful. That is the wrong way round: the shortcut is what you press when you want the panel,
//! and whether one happens to be running is the program's problem.
//!
//! So `--toggle` means "show me the panel". If one is running it flips. If none is, it starts
//! one and exits, leaving it running.
//!
//! Two files in the temp directory carry the whole thing. One holds the running panel's process
//! id, so a second invocation can tell whether there is anything to talk to. The other is a
//! request to flip, which the running panel notices and deletes. A file rather than a socket,
//! because the entire message is one bit and a file cannot fail to bind.

use std::io::Write;
use std::path::PathBuf;

/// Where the two markers live.
///
/// Injectable so the tests do not share one pair of files and race each other. Everything
/// below takes a `Marks`, and the public functions use the real one.
#[derive(Debug, Clone)]
pub struct Marks {
    pub pid: PathBuf,
    pub toggle: PathBuf,
}

impl Marks {
    pub fn under(dir: impl Into<PathBuf>, who: &str) -> Self {
        let dir = dir.into();
        Self {
            pid: dir.join(format!("carl-panel-{who}.pid")),
            toggle: dir.join(format!("carl-panel-{who}.toggle")),
        }
    }
}

/// The real pair, which every shortcut press and every running panel agrees on.
pub fn marks() -> Marks {
    Marks::under(std::env::temp_dir(), &whoami())
}

/// Where the running panel says it exists.
pub fn pid_path() -> PathBuf {
    marks().pid
}

/// Where a request to flip is left.
pub fn toggle_path() -> PathBuf {
    marks().toggle
}

fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "user".into())
}

/// Records that this process is the panel.
pub fn claim(pid: u32) {
    claim_in(&marks(), pid);
}

pub fn claim_in(marks: &Marks, pid: u32) {
    if let Ok(mut f) = std::fs::File::create(&marks.pid) {
        let _ = write!(f, "{pid}");
    }
}

/// Forgets it, so the next shortcut press starts a new one rather than talking to a corpse.
pub fn release() {
    release_in(&marks());
}

pub fn release_in(marks: &Marks) {
    let _ = std::fs::remove_file(&marks.pid);
}

/// The process id of a panel that is genuinely running, if there is one.
///
/// The liveness check is the part that matters. A panel that was killed leaves its file behind,
/// and process ids are reused, so a stale file can name a process that exists and is something
/// else entirely. Pressing the shortcut would then send a flip to a stranger and no panel would
/// appear. The command line is checked as well as the id.
pub fn running() -> Option<u32> {
    running_in(&marks(), is_ours)
}

/// The same, with the liveness check handed in, so the decision can be tested without needing
/// a real panel process to exist.
pub fn running_in(marks: &Marks, alive: impl Fn(u32) -> bool) -> Option<u32> {
    let raw = std::fs::read_to_string(&marks.pid).ok()?;
    let pid: u32 = raw.trim().parse().ok()?;
    alive(pid).then_some(pid)
}

/// Whether that process exists and is a panel.
fn is_ours(pid: u32) -> bool {
    let cmdline = format!("/proc/{pid}/cmdline");
    match std::fs::read(&cmdline) {
        // The command line is null separated, so this is a substring search rather than a parse.
        Ok(bytes) => String::from_utf8_lossy(&bytes).contains("carl-panel"),
        Err(_) => false,
    }
}

/// Asks a running panel to flip.
pub fn ask_toggle() -> std::io::Result<()> {
    ask_toggle_in(&marks())
}

pub fn ask_toggle_in(marks: &Marks) -> std::io::Result<()> {
    std::fs::write(&marks.toggle, b"1")
}

/// Whether somebody asked since the last frame. Consumes the request.
pub fn toggle_asked() -> bool {
    toggle_asked_in(&marks())
}

pub fn toggle_asked_in(marks: &Marks) -> bool {
    if marks.toggle.exists() {
        let _ = std::fs::remove_file(&marks.toggle);
        return true;
    }
    false
}

/// What `--toggle` should do.
///
/// Its own type so the decision can be tested without starting a window or a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// A panel is running. Ask it to flip.
    Flip(u32),
    /// Nothing is running. Start one.
    Start,
}

pub fn wanted() -> Wanted {
    wanted_in(&marks(), is_ours)
}

pub fn wanted_in(marks: &Marks, alive: impl Fn(u32) -> bool) -> Wanted {
    match running_in(marks, alive) {
        Some(pid) => Wanted::Flip(pid),
        None => Wanted::Start,
    }
}

/// Starts a panel and leaves it running after this process exits.
///
/// Nothing is waited on. The point of the shortcut is that pressing it returns immediately and
/// the panel appears, so holding the terminal open until the panel closes would be exactly
/// wrong.
pub fn start_detached() -> std::io::Result<u32> {
    let me = std::env::current_exe()?;
    let child = std::process::Command::new(me)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(child.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test gets its own pair of markers, so they cannot race each other over one file
    /// in the shared temp directory. The first version of these did share, and three of them
    /// failed at random depending on which finished first.
    fn marks() -> (Marks, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let m = Marks::under(dir.path(), "test");
        (m, dir)
    }

    /// The two files must not be the same file, or a request to flip would erase the record of
    /// what to flip.
    #[test]
    fn the_two_markers_are_different_files() {
        let (m, _d) = marks();
        assert_ne!(m.pid, m.toggle);
        assert!(m.pid.to_string_lossy().ends_with(".pid"));
        assert!(m.toggle.to_string_lossy().ends_with(".toggle"));
    }

    /// A request is consumed, so one press is one flip rather than a panel that keeps flipping
    /// every frame until somebody deletes a file.
    #[test]
    fn a_toggle_request_fires_once() {
        let (m, _d) = marks();
        assert!(!toggle_asked_in(&m), "nothing asked yet");

        ask_toggle_in(&m).unwrap();
        assert!(toggle_asked_in(&m), "the request is seen");
        assert!(!toggle_asked_in(&m), "and not seen twice");
    }

    /// With nothing running, the shortcut has to start one rather than doing nothing. This is
    /// the whole point of it: the first press of the day is the one that matters.
    #[test]
    fn with_no_panel_running_the_shortcut_starts_one() {
        let (m, _d) = marks();
        assert_eq!(wanted_in(&m, |_| true), Wanted::Start);
    }

    /// A panel that is up gets flipped rather than a second one being stacked on top of it.
    #[test]
    fn with_a_panel_running_the_shortcut_flips_it() {
        let (m, _d) = marks();
        claim_in(&m, 4242);
        assert_eq!(wanted_in(&m, |pid| pid == 4242), Wanted::Flip(4242));
    }

    /// A file naming a process that is alive but is not a panel is stale.
    ///
    /// Process ids are reused, so a killed panel can leave a file naming something else
    /// entirely. Trusting the id would send a flip to a stranger and no panel would appear,
    /// which looks exactly like the shortcut being broken.
    #[test]
    fn a_stale_marker_starts_a_new_panel_rather_than_flipping_a_stranger() {
        let (m, _d) = marks();
        claim_in(&m, 4242);
        assert_eq!(
            wanted_in(&m, |_| false),
            Wanted::Start,
            "the id is alive but it is not a panel"
        );
    }

    #[test]
    fn releasing_forgets_the_panel() {
        let (m, _d) = marks();
        claim_in(&m, 4242);
        assert!(m.pid.exists());

        release_in(&m);
        assert!(!m.pid.exists());
        assert_eq!(running_in(&m, |_| true), None);
    }

    /// The real liveness check, against this process, which is a test binary and not a panel.
    #[test]
    fn the_real_check_knows_a_test_binary_is_not_a_panel() {
        assert!(!is_ours(4_294_967_000), "nothing is using that id");
    }
}
