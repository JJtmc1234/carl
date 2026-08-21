//! Telling a process apart from the one that took its place.
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
//! Parsing that file has one trap worth being careful about, and it is why this is not a
//! `split_whitespace().nth(21)`. Field two is the executable name in brackets, unescaped, so a
//! program called `my prog) 0 0 0` puts whatever it likes in the middle of the line. Everything
//! is therefore counted from the last closing bracket rather than from the start.

use std::path::PathBuf;

/// When one process started, in clock ticks since boot.
///
/// Meaningless as a duration and not meant to be one. It is an identifier that happens to be a
/// number, and comparing two of them for equality is the only thing it is for.
pub type Started = u64;

/// Reads when a process started, or `None` if it has gone.
pub fn read(pid: u32) -> Option<Started> {
    let path: PathBuf = format!("/proc/{pid}/stat").into();
    parse(&std::fs::read_to_string(path).ok()?)
}

/// Pulls field 22 out of a `stat` line.
pub fn parse(line: &str) -> Option<Started> {
    // Everything after the last bracket, because the name between the brackets can contain
    // anything at all including brackets and spaces.
    let after = &line[line.rfind(')')? + 1..];

    // The first field after the name is `state`, which is field 3. So starttime, field 22, is
    // the twentieth thing here.
    after.split_whitespace().nth(19)?.parse().ok()
}

/// Whether the process at `pid` is still the one that started at `started`.
///
/// The whole point of the module in one function. A pid that is gone answers false, and so does
/// a pid that has been reused, which is the case a bare `kill -0` cannot tell apart.
pub fn is_still(pid: u32, started: Started) -> bool {
    read(pid) == Some(started)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real line, from a real `/proc/<pid>/stat`, trimmed to the fields that matter.
    const REAL: &str = "1234 (bash) S 1200 1234 1234 34816 1234 4194304 3000 500 0 0 \
                        7 3 0 0 20 0 1 0 987654 12345678 900 18446744073709551615";

    #[test]
    fn a_stat_line_gives_up_its_start_time() {
        assert_eq!(parse(REAL), Some(987654));
    }

    /// The trap this module exists for. A process free to name itself is a process free to
    /// write anything into the middle of the line, and counting from the front believes it.
    #[test]
    fn a_program_named_to_break_the_parse_does_not_break_it() {
        let hostile = REAL.replace("(bash)", "(evil) 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0)");
        assert_eq!(parse(&hostile), Some(987654));
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
        let started = read(me).expect("this process is running");
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
}
