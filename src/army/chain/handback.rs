//! Reading what a worker reported and what a reviewer decided.
//!
//! Both are free text from a model, so both are read forgivingly about shape and strictly
//! about meaning. Nora is asked for four headings and may well put a hash or a bold marker in
//! front of them. That is the same answer.
//!
//! The one place this is deliberately unforgiving is the verdict. A review that does not
//! clearly say accept is not an acceptance. Defaulting the other way would mean a rambling
//! answer with no decision in it silently passes work nobody approved, which is the exact
//! failure the review exists to prevent.

/// What Nora sends back up to Mason.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handback {
    pub did: String,
    pub verified: String,
    pub found: String,
    /// The blocker, when there genuinely is one.
    pub blocked: Option<String>,
    /// Everything she said, kept whole so a review can be checked against it.
    pub raw: String,
}

impl Handback {
    /// The form a reviewer is shown.
    pub fn summary(&self) -> String {
        let mut out = format!("DID:\n{}\n\nVERIFIED:\n{}\n", self.did, self.verified);
        if !self.found.trim().is_empty() {
            out.push_str(&format!("\nFOUND:\n{}\n", self.found));
        }
        if let Some(b) = &self.blocked {
            out.push_str(&format!("\nBLOCKED:\n{b}\n"));
        }
        out
    }
}

/// The headings Nora is asked for, in the order they are expected.
const HEADINGS: [&str; 4] = ["DID", "VERIFIED", "FOUND", "BLOCKED"];

pub fn read_handback(said: &str) -> Handback {
    let mut parts = [String::new(), String::new(), String::new(), String::new()];
    let mut current: Option<usize> = None;

    for line in said.lines() {
        match heading_on(line) {
            Some(which) => current = Some(which),
            None => {
                if let Some(which) = current {
                    parts[which].push_str(line);
                    parts[which].push('\n');
                }
            }
        }
    }

    let tidy = |s: &String| s.trim().to_string();
    let blocked = tidy(&parts[3]);

    Handback {
        did: if current.is_none() {
            // No headings at all. Better to carry the whole answer as what she did than to
            // hand a reviewer four empty sections and let it approve nothing.
            said.trim().to_string()
        } else {
            tidy(&parts[0])
        },
        verified: tidy(&parts[1]),
        found: tidy(&parts[2]),
        blocked: if is_none(&blocked) {
            None
        } else {
            Some(blocked)
        },
        raw: said.trim().to_string(),
    }
}

/// Which heading this line is, if it is one at all.
///
/// A line is a heading only when it is nothing but the word and its punctuation. A sentence
/// that happens to contain "verified" is prose about the work and belongs in the section it
/// was written in.
fn heading_on(line: &str) -> Option<usize> {
    let bare = line
        .trim()
        .trim_start_matches(['#', '*', '-', '>'])
        .trim()
        // Handles `**DID:**` in one pass, since every trailing marker is in the set.
        .trim_end_matches(['*', ':'])
        .trim();

    HEADINGS.iter().position(|h| bare.eq_ignore_ascii_case(h))
}

fn is_none(s: &str) -> bool {
    let t = s.trim().trim_end_matches('.').trim();
    t.is_empty() || t.eq_ignore_ascii_case("none") || t.eq_ignore_ascii_case("no blockers")
}

/// The heading an agent is asked to put its verification conditions under.
const DONE_WHEN: &str = "DONE WHEN";

/// Pulls out the conditions an assigner said had to be true, and the goal without them.
///
/// A task cannot be created without at least one condition, so this is what stands between a
/// model that writes an objective and a `Verification` that refuses an empty list. Returning
/// the goal separately matters: leaving "DONE WHEN" and its bullets inside the goal means the
/// next agent down reads the conditions as part of what it was asked to do.
pub fn read_must(said: &str) -> (String, Vec<String>) {
    let mut goal = Vec::new();
    let mut must = Vec::new();
    let mut inside = false;

    for line in said.lines() {
        if is_done_when(line) {
            inside = true;
            continue;
        }
        if !inside {
            goal.push(line);
            continue;
        }
        let item = line
            .trim()
            .trim_start_matches(['-', '*', '#', '>'])
            .trim_start_matches(|c: char| c.is_ascii_digit())
            .trim_start_matches(['.', ')'])
            .trim();
        if !item.is_empty() {
            must.push(item.to_string());
        }
    }

    (goal.join("\n").trim().to_string(), must)
}

fn is_done_when(line: &str) -> bool {
    let bare = line
        .trim()
        .trim_start_matches(['#', '*', '-', '>'])
        .trim()
        .trim_end_matches(['*', ':'])
        .trim();
    bare.eq_ignore_ascii_case(DONE_WHEN)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject,
}

impl Verdict {
    pub fn accepted(self) -> bool {
        self == Verdict::Accept
    }
}

/// Reads a reviewer's decision and the reason for it.
///
/// Only the first line that says anything is searched for the word, which is what
/// [`words::mason_reviews`](super::words::mason_reviews) actually asks for: the verdict as the
/// very first word on the first line. Everything after it is reason text and nothing more.
///
/// The bound matters more than it looks. This used to search every line, so a review whose
/// first line said the belt count was still wrong and whose second line began "Accepted
/// criteria were not met" came back as an acceptance, and the chain advanced Nora's task to
/// Accepted and told JJ it had passed. A reviewer writing prose is the normal case, not a
/// strange one, and prose is full of sentences that start with the word accept. See bug 14.
pub fn read_verdict(said: &str) -> (Verdict, String) {
    let mut verdict = None;
    let mut why = Vec::new();
    let mut decided = false;

    for line in said.lines() {
        let bare = line.trim().trim_matches(['*', '#', '`', ' ']).trim();
        // Leading blank lines do not count as the first line. A reviewer who starts with an
        // empty line has still put the verdict on the first line they wrote.
        if !decided && !bare.is_empty() {
            decided = true;
            let first = bare
                .split(|c: char| !c.is_ascii_alphabetic())
                .find(|w| !w.is_empty())
                .unwrap_or("");
            if first.eq_ignore_ascii_case("accept") || first.eq_ignore_ascii_case("accepted") {
                verdict = Some(Verdict::Accept);
                push_tail(&mut why, bare, first);
                continue;
            }
            if first.eq_ignore_ascii_case("reject") || first.eq_ignore_ascii_case("rejected") {
                verdict = Some(Verdict::Reject);
                push_tail(&mut why, bare, first);
                continue;
            }
        }
        if !bare.is_empty() {
            why.push(bare.to_string());
        }
    }

    let reason = why.join(" ").trim().to_string();
    match verdict {
        Some(v) => (v, reason),
        // Said nothing decisive, so nothing is approved. The reason carries the whole answer,
        // because whoever reads this needs to see what was said instead of a decision.
        None => (
            Verdict::Reject,
            format!("no clear accept or reject in the review: {reason}"),
        ),
    }
}

/// Whatever followed the verdict word on the same line, which is often the whole reason.
///
/// The word is located rather than assumed to start at the beginning. A reviewer who numbers
/// the line writes "1. REJECT because ...", and slicing from the word's length instead of from
/// where it actually sits cuts the reason mid word. With anything non ASCII on the line that
/// same slice can land inside a character and panic, which is a review crashing the run rather
/// than being read.
fn push_tail(why: &mut Vec<String>, line: &str, word: &str) {
    let Some(at) = line.find(word) else {
        return;
    };
    let tail = line[at + word.len()..]
        .trim_start_matches([':', '.', ',', ' ', '-', '*'])
        .trim();
    if !tail.is_empty() {
        why.push(tail.to_string());
    }
}
