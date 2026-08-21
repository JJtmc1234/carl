//! Telling a process apart from the one that took its place, and from its own corpse.
//!
//! A pid is not a name. Linux hands them out in order and wraps around, so a pid recorded an
//! hour ago and found alive now may belong to something else entirely. For a panel counting
//! processes that hardly matters. For a supervisor deciding whether the agent it started is
//! still running, it is the difference between leaving an agent alone and adopting a stranger,
//! and then between killing a stale record and killing somebody's editor.
//!
//! The kernel already solves this. `/proc/<pid>/stat` field 22 is the time the process started,
//! in clock ticks since boot, and it is fixed for the life of that process. A pid and a start
//! time together name one process for as long as the machine is up, which is exactly the
//! lifetime a supervisor's record needs to cover.
//!
//! **A zombie is not running, and this is where that has to be caught.** A child that has exited
//! and not yet been reaped still has a `/proc` entry, still has its pid, and still reports the
//! same start time, because it is the same process and it is finished. A supervisor that only
//! compared start times would watch an agent exit and see it as healthy forever, since it is the
//! supervisor's own failure to reap that keeps the entry alive. That was not a hypothesis; it is
//! what the first run of the supervisor tests did.
//!
//! Parsing that file has one more trap, and it is why this is not a `split_whitespace().nth(21)`.
//! Field two is the executable name in brackets, unescaped, so a program called `my prog) 0 0 0`
//! puts whatever it likes in the middle of the line. Everything is counted from the last closing
//! bracket rather than from the start.

use std::path::PathBuf;

/// When one process started, in clock ticks since boot.
///
/// Meaningless as a duration and not meant to be one. It is an identifier that happens to be a
/// number, and comparing two of them for equality is the only thing it is for.
pub type Started = u64;

/// The two fields of `stat` that say which process this is and whether it is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    /// The kernel's single letter. `Z` is the one that matters here.
    pub state: char,
    pub started: Started,
}

impl Stamp {
    /// A process that has exited and not been reaped. Present in `/proc`, and not running.
    pub fn is_zombie(&self) -> bool {
        self.state == 'Z'
    }
}

/// Reads a process's stamp, or `None` if it has gone entirely.
pub fn read(pid: u32) -> Option<Stamp> {
    let path: PathBuf = format!("/proc/{pid}/stat").into();
    parse(&std::fs::read_to_string(path).ok()?)
}

/// Just the start time, for a caller that has already decided the process is alive.
pub fn started(pid: u32) -> Option<Started> {
    read(pid).map(|s| s.started)
}

/// Pulls the state and field 22 out of a `stat` line.
pub fn parse(line: &str) -> Option<Stamp> {
    // Everything after the last bracket, because the name between the brackets can contain
    // anything at all including brackets and spaces.
    let after = &line[line.rfind(')')? + 1..];
    let mut fields = after.split_whitespace();

    // The first field after the name is `state`, field 3. Counting from there, starttime is
    // field 22 and so the nineteenth of what is left.
    let state = fields.next()?.chars().next()?;
    let started = fields.nth(18)?.parse().ok()?;
    Some(Stamp { state, started })
}

/// Whether the process at `pid` is still the running process that started at `started`.
///
/// Three ways to answer no and they are all no: it has gone, its pid has been reused, or it has
/// exited and is waiting to be reaped.
pub fn is_still(pid: u32, started: Started) -> bool {
    read(pid).is_some_and(|s| s.started == started && !s.is_zombie())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real line, from a real `/proc/<pid>/stat`, trimmed to the fields that matter.
    const REAL: &str = "1234 (bash) S 1200 1234 1234 34816 1234 4194304 3000 500 0 0 \
                        7 3 0 0 20 0 1 0 987654 12345678 900 18446744073709551615";

    #[test]
    fn a_stat_line_gives_up_its_state_and_start_time() {
        let stamp = parse(REAL).unwrap();
        assert_eq!(stamp.started, 987654);
        assert_eq!(stamp.state, 'S');
        assert!(!stamp.is_zombie());
    }

    /// The trap this module exists for. A process free to name itself is a process free to
    /// write anything into the middle of the line, and counting from the front believes it.
    #[test]
    fn a_program_named_to_break_the_parse_does_not_break_it() {
        let hostile = REAL.replace("(bash)", "(evil) 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)");
        assert_eq!(parse(&hostile).unwrap().started, 987654);
    }

    #[test]
    fn a_line_that_is_not_a_stat_line_gives_nothing() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("nonsense"), None);
        assert_eq!(parse("1 (short) S 0 0"), None, "too few fields");
    }

    /// This process is the one thing guaranteed to be there, so it is what the reader is
    /// checked against rather than a pid picked out of the air.
    #[test]
    fn this_process_has_a_start_time_and_is_still_itself() {
        let me = std::process::id();
        let started = started(me).expect("this process is running");
        assert!(is_still(me, started));
        assert!(!is_still(me, started + 1), "a different process, same pid");
    }

    /// A pid nobody is using answers absent rather than failing, because a supervisor asking
    /// about a process that has exited is the normal case and not an error.
    #[test]
    fn a_pid_that_is_not_running_is_absent_rather_than_an_error() {
        // Above the configured maximum, so it cannot be in use.
        assert_eq!(read(u32::MAX), None);
        assert!(!is_still(u32::MAX, 1));
    }

    /// The one that a start time alone gets wrong. A child that has exited and not been reaped
    /// keeps its pid, its /proc entry and its start time, and it is not running.
    #[test]
    fn a_zombie_is_not_still_running() {
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("true is on every machine");
        let pid = child.id();

        // Wait for it to finish without reaping it, which is exactly the state a supervisor
        // holding a Child finds its agent in a microsecond after the agent exits.
        let started = loop {
            match read(pid) {
                Some(s) if s.is_zombie() => break s.started,
                Some(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                None => panic!("it was reaped by somebody else"),
            }
        };

        assert_eq!(
            parse(&std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap()),
            read(pid),
            "and the file still parses"
        );
        assert!(!is_still(pid, started), "a zombie has exited");

        child.wait().unwrap();
    }
}
