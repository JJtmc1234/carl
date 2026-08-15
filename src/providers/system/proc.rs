//! The numbers Linux hands out for free, parsed.
//!
//! Everything here reads a file under `/proc` that is world readable on this machine. No sudo,
//! no helper daemon, no capabilities. Each source was checked on JJ's machine before it was
//! written down, and anything that could not be checked is not in this file.
//!
//! The parsers take text rather than paths so they can be tested against a fixed sample. The
//! readers are the thin part on top that goes and gets the text, because a parser that can only
//! be tested by having the right hardware is a parser nobody tests.

/// Where the kernel publishes what it is doing.
pub const STAT: &str = "/proc/stat";
pub const MEMINFO: &str = "/proc/meminfo";
pub const LOADAVG: &str = "/proc/loadavg";
pub const UPTIME: &str = "/proc/uptime";
pub const NET_DEV: &str = "/proc/net/dev";

/// Busy and idle jiffies since boot, which are only meaningful as a difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
}

impl CpuTimes {
    /// What fraction of the time between two readings was spent busy.
    ///
    /// Returns `None` when the counters did not move or went backwards. A machine that has not
    /// ticked between two samples has no utilisation to report, and inventing zero there would
    /// show an idle CPU on a busy one.
    pub fn busy_fraction_since(&self, earlier: &CpuTimes) -> Option<f64> {
        let total = self.total.checked_sub(earlier.total)?;
        let idle = self.idle.checked_sub(earlier.idle)?;
        if total == 0 || idle > total {
            return None;
        }
        Some((total - idle) as f64 / total as f64)
    }
}

/// Reads the aggregate `cpu` line. The per core lines below it are deliberately ignored,
/// because this is a panel rather than htop.
pub fn parse_cpu_times(stat: &str) -> Option<CpuTimes> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|f| f.parse().ok())
        .collect();
    if fields.len() < 5 {
        return None;
    }
    // user nice system idle iowait irq softirq steal ...
    // Idle counts iowait too, because a core waiting on a disk is not doing work.
    let idle = fields[3] + fields[4];
    Some(CpuTimes {
        total: fields.iter().sum(),
        idle,
    })
}

/// The handful of `/proc/meminfo` keys worth showing, in kibibytes as the kernel gives them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemInfo {
    pub total_kb: Option<u64>,
    pub available_kb: Option<u64>,
    pub swap_total_kb: Option<u64>,
    pub swap_free_kb: Option<u64>,
}

impl MemInfo {
    /// Used memory, which is total minus what the kernel says is actually available.
    ///
    /// Available rather than free on purpose. Free excludes cache, so a healthy machine using
    /// its memory as intended reads as almost out of memory.
    pub fn used_kb(&self) -> Option<u64> {
        Some(self.total_kb?.saturating_sub(self.available_kb?))
    }

    pub fn used_fraction(&self) -> Option<f64> {
        let total = self.total_kb?;
        if total == 0 {
            return None;
        }
        Some(self.used_kb()? as f64 / total as f64)
    }

    pub fn swap_used_kb(&self) -> Option<u64> {
        Some(self.swap_total_kb?.saturating_sub(self.swap_free_kb?))
    }
}

pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut out = MemInfo::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value: Option<u64> = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        match key {
            "MemTotal" => out.total_kb = value,
            "MemAvailable" => out.available_kb = value,
            "SwapTotal" => out.swap_total_kb = value,
            "SwapFree" => out.swap_free_kb = value,
            _ => {}
        }
    }
    out
}

/// The one, five and fifteen minute run queue averages.
pub fn parse_loadavg(text: &str) -> Option<[f64; 3]> {
    let mut it = text.split_whitespace();
    let one = it.next()?.parse().ok()?;
    let five = it.next()?.parse().ok()?;
    let fifteen = it.next()?.parse().ok()?;
    Some([one, five, fifteen])
}

/// Seconds since boot.
pub fn parse_uptime(text: &str) -> Option<f64> {
    text.split_whitespace().next()?.parse().ok()
}

/// Total bytes received and sent across every interface except loopback.
///
/// Loopback is excluded because it is mostly this machine talking to itself, and on a box
/// running an agent army that number is large and says nothing about the network.
pub fn parse_net_dev(text: &str) -> Option<(u64, u64)> {
    let mut rx = 0u64;
    let mut tx = 0u64;
    let mut saw_one = false;

    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();
        if fields.len() >= 9 {
            rx += fields[0];
            tx += fields[8];
            saw_one = true;
        }
    }
    saw_one.then_some((rx, tx))
}

/// Reads a file, treating anything unreadable as absent rather than as an error.
///
/// A diagnostics provider that fails because one optional source is missing is a diagnostics
/// provider that stops working exactly when the machine is unhappy.
pub fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL_STAT: &str = "cpu  54491251 39343 15875108 292220940 33144150 0 645197 0 0 0\n\
                             cpu0 100 1 2 3 4 0 0 0 0 0\n\
                             intr 1 2 3\n";

    #[test]
    fn the_aggregate_cpu_line_is_the_one_that_is_read() {
        let t = parse_cpu_times(REAL_STAT).unwrap();
        assert_eq!(t.idle, 292220940 + 33144150, "iowait counts as idle");
        assert_eq!(
            t.total,
            54491251 + 39343 + 15875108 + 292220940 + 33144150 + 645197
        );
    }

    #[test]
    fn utilisation_is_the_difference_between_two_readings() {
        let earlier = CpuTimes {
            total: 1000,
            idle: 900,
        };
        let later = CpuTimes {
            total: 1100,
            idle: 950,
        };
        let busy = later.busy_fraction_since(&earlier).unwrap();
        assert!(
            (busy - 0.5).abs() < 1e-9,
            "50 of 100 jiffies were busy, got {busy}"
        );
    }

    /// A counter that has not moved has no utilisation to report, and zero would read as idle.
    #[test]
    fn a_stalled_or_backwards_counter_reports_nothing_rather_than_zero() {
        let a = CpuTimes {
            total: 1000,
            idle: 900,
        };
        assert_eq!(a.busy_fraction_since(&a), None, "no time passed");

        let earlier = CpuTimes {
            total: 2000,
            idle: 1000,
        };
        assert_eq!(
            a.busy_fraction_since(&earlier),
            None,
            "counters went backwards"
        );
    }

    #[test]
    fn a_stat_file_without_a_cpu_line_parses_to_nothing() {
        assert_eq!(parse_cpu_times("intr 1 2 3\n"), None);
        assert_eq!(parse_cpu_times(""), None);
        assert_eq!(
            parse_cpu_times("cpu  1 2\n"),
            None,
            "too few fields to trust"
        );
    }

    const REAL_MEMINFO: &str = "MemTotal:       63941640 kB\n\
                                MemFree:         8360516 kB\n\
                                MemAvailable:   32879140 kB\n\
                                SwapTotal:       8388604 kB\n\
                                SwapFree:        8218220 kB\n";

    #[test]
    fn memory_is_read_as_total_minus_available() {
        let m = parse_meminfo(REAL_MEMINFO);
        assert_eq!(m.total_kb, Some(63941640));
        assert_eq!(m.used_kb(), Some(63941640 - 32879140));
        let used = m.used_fraction().unwrap();
        assert!((0.48..0.49).contains(&used), "about half used, got {used}");
        assert_eq!(m.swap_used_kb(), Some(8388604 - 8218220));
    }

    /// The missing metric rule, at the parser level.
    #[test]
    fn meminfo_without_the_keys_reports_nothing_rather_than_zero() {
        let m = parse_meminfo("Something: 1 kB\n");
        assert_eq!(m.total_kb, None);
        assert_eq!(m.used_kb(), None);
        assert_eq!(m.used_fraction(), None);
        assert_eq!(m.swap_used_kb(), None);
    }

    #[test]
    fn a_machine_with_no_swap_reports_zero_used_rather_than_unknown() {
        let m = parse_meminfo("SwapTotal: 0 kB\nSwapFree: 0 kB\n");
        assert_eq!(
            m.swap_used_kb(),
            Some(0),
            "no swap is a real answer, not a gap"
        );
    }

    #[test]
    fn load_and_uptime_parse() {
        assert_eq!(
            parse_loadavg("4.90 5.17 5.24 9/3214 2888406"),
            Some([4.90, 5.17, 5.24])
        );
        assert_eq!(parse_loadavg("nonsense"), None);
        assert_eq!(parse_uptime("791794.08 2922209.55"), Some(791794.08));
        assert_eq!(parse_uptime(""), None);
    }

    const REAL_NET: &str = "Inter-|   Receive                        |  Transmit\n\
         face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n\
            lo: 55849670105 959006803 0 0 0 0 0 0 55849670105 959006803 0 0 0 0 0 0\n\
          wlo1: 54616834862 55413755 0 1 0 0 0 0 27332584431 25183292 0 667 0 0 0 0\n";

    #[test]
    fn network_totals_skip_loopback() {
        let (rx, tx) = parse_net_dev(REAL_NET).unwrap();
        assert_eq!(
            rx, 54616834862,
            "loopback is this machine talking to itself"
        );
        assert_eq!(tx, 27332584431);
    }

    #[test]
    fn a_net_file_with_no_real_interfaces_reports_nothing() {
        assert_eq!(
            parse_net_dev("Inter-|\n face |\n    lo: 1 2 3 4 5 6 7 8 9\n"),
            None
        );
    }

    #[test]
    fn an_unreadable_file_is_absent_rather_than_an_error() {
        assert_eq!(read("/proc/definitely-not-a-real-file"), None);
        assert!(read(STAT).is_some(), "and the real one is there");
    }
}
