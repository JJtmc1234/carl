//! What one process is costing, and whether it is still there.
//!
//! Everything here has to survive the process disappearing between one line and the next,
//! because that is the normal case rather than the edge. A panel that samples Carl's `claude`
//! subprocesses is reading a list that changes while it is being read, and the honest answer
//! for a process that has exited is that it is gone, not that it is using no memory.

use std::path::PathBuf;

/// A snapshot of one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub name: String,
    /// The single letter the kernel uses. `R` running, `S` sleeping, `Z` zombie, `D` waiting
    /// on disk.
    pub state: String,
    pub resident_kb: Option<u64>,
    pub threads: Option<u64>,
}

impl Process {
    /// A zombie is a process that has exited and not been reaped. It costs nothing and means
    /// whoever started it is not watching, which is worth saying out loud.
    pub fn is_zombie(&self) -> bool {
        self.state == "Z"
    }
}

/// Parses `/proc/<pid>/status`.
pub fn parse_status(pid: u32, text: &str) -> Option<Process> {
    let mut name = None;
    let mut state = None;
    let mut resident_kb = None;
    let mut threads = None;

    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let rest = rest.trim();
        match key {
            "Name" => name = Some(rest.to_string()),
            // "S (sleeping)", and only the letter is worth keeping.
            "State" => state = rest.split_whitespace().next().map(str::to_string),
            "VmRSS" => resident_kb = rest.split_whitespace().next().and_then(|v| v.parse().ok()),
            "Threads" => threads = rest.parse().ok(),
            _ => {}
        }
    }

    Some(Process {
        pid,
        name: name?,
        state: state?,
        // A kernel thread has no VmRSS at all, and neither does a zombie. Absent rather than
        // zero, because zero resident memory is a claim and this is a gap.
        resident_kb,
        threads,
    })
}

/// Reads one process, or `None` if it has gone.
pub fn read(pid: u32) -> Option<Process> {
    let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_status(pid, &text)
}

/// Every process owned by this user whose name matches, newest first is not promised.
///
/// Reads `/proc` directly rather than shelling out to `pgrep`, so there is no dependency on
/// which `procps` is installed. Entries that vanish mid scan are skipped rather than failing
/// the whole listing.
pub fn find_by_name(wanted: &str) -> Vec<Process> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path: PathBuf = entry.path();
        let Some(pid) = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        // The race this whole module is built around: between listing and reading, the process
        // may exit. That is a skip, not an error.
        if let Some(p) = read(pid).filter(|p| p.name == wanted) {
            out.push(p);
        }
    }
    out.sort_by_key(|p| p.pid);
    out
}

/// This process, which always exists and is therefore the one thing that can be asserted on.
pub fn own() -> Option<Process> {
    read(std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = "Name:\tbash\nUmask:\t0022\nState:\tS (sleeping)\n\
                        Tgid:\t1234\nPid:\t1234\nVmRSS:\t    7632 kB\nThreads:\t2\n";

    #[test]
    fn a_status_file_parses_into_a_process() {
        let p = parse_status(1234, REAL).unwrap();
        assert_eq!(p.name, "bash");
        assert_eq!(p.state, "S", "only the letter, not the word in brackets");
        assert_eq!(p.resident_kb, Some(7632));
        assert_eq!(p.threads, Some(2));
        assert!(!p.is_zombie());
    }

    /// A kernel thread or a zombie has no resident memory line at all, and that is a gap
    /// rather than a measurement of zero.
    #[test]
    fn a_process_without_resident_memory_reports_unknown_rather_than_zero() {
        let p = parse_status(2, "Name:\tkthreadd\nState:\tS (sleeping)\nThreads:\t1\n").unwrap();
        assert_eq!(p.resident_kb, None);
        assert_eq!(p.threads, Some(1));
    }

    #[test]
    fn a_zombie_is_recognised() {
        let p = parse_status(9, "Name:\tgone\nState:\tZ (zombie)\n").unwrap();
        assert!(p.is_zombie());
    }

    #[test]
    fn a_status_file_missing_the_essentials_parses_to_nothing() {
        assert_eq!(parse_status(1, "Umask:\t0022\n"), None);
        assert_eq!(parse_status(1, "Name:\tthing\n"), None, "no state");
    }

    /// Against the real machine.
    #[test]
    fn this_process_can_read_itself() {
        let me = own().expect("this process exists");
        assert_eq!(me.pid, std::process::id());
        assert!(me.resident_kb.unwrap_or(0) > 0, "we are using some memory");
        assert!(!me.is_zombie(), "not yet");
    }

    /// The race, forced. A pid that has certainly exited must read as gone.
    #[test]
    fn a_process_that_has_gone_reads_as_gone_rather_than_empty() {
        let child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        let pid = child.id();
        let mut child = child;
        child.wait().expect("reap it");

        // Reaped, so even the zombie entry is gone. Anything else would be a fabricated
        // reading for a process that does not exist.
        assert_eq!(read(pid), None);
    }

    #[test]
    fn a_pid_that_could_never_exist_is_gone() {
        assert_eq!(read(u32::MAX), None);
    }

    #[test]
    fn searching_for_a_name_nothing_has_finds_nothing() {
        assert!(find_by_name("definitely-not-a-real-process-name-xyzzy").is_empty());
    }
}
