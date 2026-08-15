//! A whole job, from a request to an answer, through three tiers.
//!
//! ```text
//!   JJ asks for something
//!     |
//!   Carl              picks the departments and writes each head a brief
//!     |               and writes nothing else, ever
//!     +-- head        splits its brief into jobs
//!     |     +-- worker    does one job, proposes, applies nothing
//!     |     +-- worker
//!     |   head        reads what came back and decides
//!     +-- head        the same, in parallel
//!     |
//!   Carl              only if the heads disagree, and only then
//! ```
//!
//! The shape follows what JJ asked for and every rule in it is load bearing.
//!
//! Carl never writes the work. He decomposes and delegates, and the single thing he decides
//! about content is a tie, after both heads have made their case. A chief executive who
//! rewrites his staff's output is a bottleneck with a title, and the whole answer collapses
//! back into one model's opinion wearing twenty hats.
//!
//! Heads decide inside their department and nothing goes upward that they could settle. That
//! is what stops every small disagreement becoming Carl's problem, which is what makes twenty
//! agents cost less attention than one.
//!
//! Workers cannot apply anything. Twenty agents editing one repository at once is the failure
//! that eats a day, and permission living with the head is what prevents it.

use std::time::Duration;

use super::{Army, Dispatch, Report, report, roster};
use crate::Result;

/// How many jobs a head may split its brief into.
///
/// Enough to be worth having a department, few enough that one head reading all of them is
/// still reading rather than skimming.
pub const JOBS_PER_HEAD: usize = 3;

/// What one department produced.
#[derive(Debug, Clone)]
pub struct Departmental {
    pub head: String,
    /// What the head decided, having read its workers.
    pub decision: String,
    /// Everything the workers said, kept so a decision can be checked against it.
    pub reports: Vec<Report>,
}

/// The whole run.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub departments: Vec<Departmental>,
    /// Carl's tiebreak, only present when the heads actually disagreed.
    pub tiebreak: Option<String>,
}

impl Outcome {
    /// What to show whoever asked.
    ///
    /// The departments in full, then the tiebreak if there was one. Nothing is summarised
    /// away, because the point of asking twenty agents is the twenty answers and a chief
    /// executive who condenses them has just become the single opinion again.
    pub fn presented(&self) -> String {
        let mut out = String::new();
        for d in &self.departments {
            out.push_str(&format!("# {}\n\n{}\n\n", d.head, d.decision));
        }
        if let Some(tie) = &self.tiebreak {
            out.push_str(&format!("# Carl, breaking the tie\n\n{tie}\n"));
        }
        out
    }
}

/// Runs one department: the head splits, the workers work, the head decides.
///
/// Two calls to the head and one per worker. The head's first call is the only place that
/// knows how this particular brief should be divided, and asking it rather than deciding here
/// is the difference between a department and a fixed script.
pub fn run_department(army: &Army, head: &str, brief: &str) -> Result<Departmental> {
    let Some(role) = roster::find(head) else {
        return Err(crate::Error::Refused(format!("no head called {head}")));
    };
    let under: Vec<&roster::Role> = roster::department(head)
        .into_iter()
        .filter(|r| !r.is_head())
        .collect();

    if under.is_empty() {
        return Err(crate::Error::Refused(format!("{head} leads nobody")));
    }

    // The head decides how to split its own brief. Anything else makes every department run
    // the same way regardless of what it was asked.
    let split = army.ask_one(
        role,
        &format!(
            "You have been asked to handle this:\n\n{brief}\n\nSplit it into at most \
             {JOBS_PER_HEAD} jobs for your workers. You have these workers and no others: \
             {}.\n\nAnswer with one line per job, in the form `worker-name: what they should \
             do`. Nothing else, no preamble, no numbering. Give a worker nothing rather than \
             inventing work for them.",
            under.iter().map(|r| r.name).collect::<Vec<_>>().join(", ")
        ),
    )?;

    let tasks = parse_jobs(&split, &under);
    if tasks.is_empty() {
        return Err(crate::Error::Refused(format!(
            "{head} split the work into nothing usable. It said: {}",
            split.lines().take(3).collect::<Vec<_>>().join(" ")
        )));
    }

    let reports = army.deploy(&tasks);

    // The head reads everything and decides. Failures are named rather than dropped, so a
    // decision cannot quietly claim coverage that was never there.
    let decision = army.ask_one(
        role,
        &format!(
            "You asked for this:\n\n{brief}\n\nYour workers came back with the following. \
             Decide what is actually right, settle any disagreement between them yourself, and \
             say what should happen. Approve nothing you have not understood.\n\n{}",
            report::gather(&reports)
        ),
    )?;

    Ok(Departmental {
        head: head.to_string(),
        decision,
        reports,
    })
}

/// Reads a head's split into jobs.
///
/// Forgiving about shape and strict about names. A head that answers with a bullet or a number
/// in front of the worker meant the same thing, and a head that names somebody who does not
/// work for it did not.
pub fn parse_jobs(said: &str, under: &[&roster::Role]) -> Vec<Dispatch> {
    let mut tasks = Vec::new();

    for line in said.lines() {
        let line = line
            .trim()
            .trim_start_matches(['-', '*', '#', '>'])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')'])
            .trim();

        let Some((name, what)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().trim_matches(['`', '*']).to_lowercase();
        let what = what.trim();
        if what.is_empty() {
            continue;
        }

        if let Some(role) = under.iter().find(|r| r.name == name) {
            tasks.push(Dispatch::new(role.name, what));
        }
    }

    tasks.truncate(JOBS_PER_HEAD);
    tasks
}

impl Army {
    /// One call to one agent, used for the heads.
    ///
    /// The same machinery a worker runs through, with a deadline, because a head that hangs
    /// stops its whole department just as surely.
    pub fn ask_one(&self, role: &roster::Role, instruction: &str) -> Result<String> {
        let reports = self
            .clone_for(Duration::from_secs(180))
            .deploy(&[Dispatch::new(role.name, instruction)]);

        match reports.into_iter().next() {
            Some(r) if r.worked() => Ok(r.said),
            Some(r) => Err(crate::Error::Refused(format!(
                "{} did not answer: {}",
                role.name,
                r.failed.unwrap_or_default()
            ))),
            None => Err(crate::Error::Refused("no report at all".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn under(names: &[&str]) -> Vec<&'static roster::Role> {
        names.iter().filter_map(|n| roster::find(n)).collect()
    }

    #[test]
    fn a_plain_split_is_read() {
        let said = "correctness: look for off by one errors\nsimplifier: find repeated work";
        let tasks = parse_jobs(said, &under(&["correctness", "simplifier", "security"]));

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].role, "correctness");
        assert_eq!(tasks[0].instruction, "look for off by one errors");
    }

    /// A head that puts a bullet or a number in front meant exactly the same thing.
    #[test]
    fn the_shape_of_the_line_does_not_matter() {
        for said in [
            "- correctness: check it",
            "1. correctness: check it",
            "* `correctness`: check it",
            "  correctness : check it  ",
        ] {
            let tasks = parse_jobs(said, &under(&["correctness"]));
            assert_eq!(tasks.len(), 1, "failed on {said:?}");
            assert_eq!(tasks[0].instruction, "check it");
        }
    }

    /// A head naming somebody who does not work for it did not mean the same thing, and
    /// running it anyway puts a job in a department that never asked for it.
    #[test]
    fn a_worker_from_another_department_is_ignored() {
        let said = "implementer: write the thing\ncorrectness: check it";
        let tasks = parse_jobs(said, &under(&["correctness", "simplifier"]));

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].role, "correctness");
    }

    #[test]
    fn prose_around_the_lines_is_skipped() {
        let said = "Here is how I would split this.\n\ncorrectness: check it\n\nThat should do.";
        assert_eq!(parse_jobs(said, &under(&["correctness"])).len(), 1);
    }

    /// A head reading four jobs is skimming rather than reading, which is the thing a head is
    /// for.
    #[test]
    fn a_head_cannot_hand_out_more_jobs_than_it_may() {
        let said = (0..10)
            .map(|i| format!("correctness: job {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            parse_jobs(&said, &under(&["correctness"])).len(),
            JOBS_PER_HEAD
        );
    }

    #[test]
    fn a_job_with_no_instruction_is_not_a_job() {
        assert!(parse_jobs("correctness:", &under(&["correctness"])).is_empty());
        assert!(parse_jobs("correctness:   ", &under(&["correctness"])).is_empty());
    }

    /// The departments in full, because the point of asking twenty agents is the twenty
    /// answers, and condensing them makes it one opinion again.
    #[test]
    fn everything_the_departments_said_is_shown() {
        let outcome = Outcome {
            departments: vec![
                Departmental {
                    head: "reviewer".into(),
                    decision: "the bounds check is wrong".into(),
                    reports: Vec::new(),
                },
                Departmental {
                    head: "verifier".into(),
                    decision: "there is no test for it".into(),
                    reports: Vec::new(),
                },
            ],
            tiebreak: None,
        };

        let shown = outcome.presented();
        assert!(shown.contains("bounds check is wrong"));
        assert!(shown.contains("no test for it"));
        assert!(!shown.contains("breaking the tie"), "there was no tie");
    }

    #[test]
    fn a_tiebreak_is_shown_when_there_was_one() {
        let outcome = Outcome {
            departments: Vec::new(),
            tiebreak: Some("go with the reviewer".into()),
        };
        assert!(outcome.presented().contains("breaking the tie"));
    }
}
