//! What came back, and whether it is worth acting on.
//!
//! Twenty agents produce twenty answers, and the difference between that being useful and
//! being a wall of text is entirely in what happens next. A report says who wrote it, what
//! they said, how long they took, and whether they failed, and a summary says whether the run
//! as a whole is worth trusting.
//!
//! The failure count is the part that matters. Nineteen good answers and one failure is a
//! result. Four good answers and sixteen failures looks identical from a distance and is not a
//! result at all, and anything reading only the successes cannot tell those apart.

use std::time::Duration;

/// What one agent produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub role: String,
    /// What it said. Empty when it failed.
    pub said: String,
    /// Why it failed, if it did.
    pub failed: Option<String>,
    pub took: Duration,
}

impl Report {
    pub fn done(role: &str, said: &str) -> Self {
        Self {
            role: role.to_string(),
            said: said.trim().to_string(),
            failed: None,
            took: Duration::ZERO,
        }
    }

    pub fn failed(role: &str, why: &str) -> Self {
        Self {
            role: role.to_string(),
            said: String::new(),
            failed: Some(why.to_string()),
            took: Duration::ZERO,
        }
    }

    pub fn after(mut self, took: Duration) -> Self {
        self.took = took;
        self
    }

    pub fn worked(&self) -> bool {
        self.failed.is_none()
    }

    /// One line, for watching a run go past.
    pub fn line(&self) -> String {
        match &self.failed {
            Some(why) => format!(
                "  {:<18} failed after {:.1}s: {why}",
                self.role,
                self.took.as_secs_f32()
            ),
            None => format!(
                "  {:<18} {:.1}s, {} words",
                self.role,
                self.took.as_secs_f32(),
                self.said.split_whitespace().count()
            ),
        }
    }
}

/// How a whole run went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub worked: usize,
    pub failed: usize,
    pub slowest: Duration,
    pub total: Duration,
}

impl Summary {
    pub fn of(reports: &[Report]) -> Self {
        Self {
            worked: reports.iter().filter(|r| r.worked()).count(),
            failed: reports.iter().filter(|r| !r.worked()).count(),
            slowest: reports.iter().map(|r| r.took).max().unwrap_or_default(),
            total: reports.iter().map(|r| r.took).sum(),
        }
    }

    /// Whether enough of the run worked to act on it.
    ///
    /// More than half, and at least one. A run where most agents failed produces an answer
    /// that reads exactly like a good one and is built on four opinions instead of twenty, and
    /// the only place that difference is visible is here.
    pub fn worth_acting_on(&self) -> bool {
        self.worked > 0 && self.worked * 2 > self.worked + self.failed
    }

    pub fn line(&self) -> String {
        format!(
            "{} of {} agents finished, slowest {:.1}s, {:.0}s of work done in parallel",
            self.worked,
            self.worked + self.failed,
            self.slowest.as_secs_f32(),
            self.total.as_secs_f32()
        )
    }
}

/// Everything that worked, gathered for whoever has to make sense of it.
///
/// Failures are named rather than dropped. An agent that could not answer is information: it
/// says that part of the question is unanswered, and leaving it out lets a synthesis quietly
/// claim coverage it does not have.
pub fn gather(reports: &[Report]) -> String {
    let mut out = String::new();

    for r in reports.iter().filter(|r| r.worked()) {
        out.push_str(&format!("## {}\n\n{}\n\n", r.role, r.said));
    }

    let failed: Vec<&str> = reports
        .iter()
        .filter(|r| !r.worked())
        .map(|r| r.role.as_str())
        .collect();

    if !failed.is_empty() {
        out.push_str(&format!(
            "## not answered\n\nThese did not report back: {}. Say so rather than writing \
             around the gap, because whatever they were looking at has not been looked at.\n\n",
            failed.join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(role: &str, secs: f32) -> Report {
        Report::done(role, "something useful").after(Duration::from_secs_f32(secs))
    }

    fn bad(role: &str) -> Report {
        Report::failed(role, "timed out").after(Duration::from_secs(30))
    }

    #[test]
    fn a_summary_counts_both_kinds() {
        let s = Summary::of(&[ok("a", 1.0), ok("b", 3.0), bad("c")]);
        assert_eq!(s.worked, 2);
        assert_eq!(s.failed, 1);
        assert_eq!(s.slowest, Duration::from_secs(30));
    }

    /// Four opinions dressed as twenty reads exactly like twenty, and this is the only place
    /// the difference is visible.
    #[test]
    fn a_run_where_most_agents_failed_is_not_worth_acting_on() {
        let mostly_bad: Vec<Report> = (0..4)
            .map(|i| ok(&format!("ok{i}"), 1.0))
            .chain((0..16).map(|i| bad(&format!("bad{i}"))))
            .collect();
        assert!(!Summary::of(&mostly_bad).worth_acting_on());

        let mostly_good: Vec<Report> = (0..19)
            .map(|i| ok(&format!("ok{i}"), 1.0))
            .chain(std::iter::once(bad("one")))
            .collect();
        assert!(Summary::of(&mostly_good).worth_acting_on());
    }

    #[test]
    fn a_run_where_nothing_worked_is_not_worth_acting_on() {
        assert!(!Summary::of(&[bad("a"), bad("b")]).worth_acting_on());
        assert!(!Summary::of(&[]).worth_acting_on());
    }

    /// An agent that could not answer is information. Leaving it out lets a synthesis claim
    /// coverage it does not have.
    #[test]
    fn what_failed_is_named_rather_than_dropped() {
        let g = gather(&[ok("power", 1.0), bad("smelting")]);
        assert!(g.contains("## power"));
        assert!(g.contains("not answered"), "{g}");
        assert!(g.contains("smelting"), "{g}");
    }

    #[test]
    fn a_run_with_no_failures_says_nothing_about_failures() {
        let g = gather(&[ok("power", 1.0)]);
        assert!(!g.contains("not answered"), "{g}");
    }

    #[test]
    fn a_line_reads_differently_for_a_failure() {
        assert!(ok("power", 2.0).line().contains("words"));
        assert!(bad("power").line().contains("failed"));
    }
}
