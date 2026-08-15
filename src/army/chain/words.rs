//! Exactly what each agent is asked, at each step.
//!
//! Kept in one place rather than spread through the runtime, because the prompts are the part
//! most likely to be wrong and the part most likely to be tuned. A run that behaves oddly is
//! usually a sentence in here rather than a bug in the machinery.
//!
//! Two shapes are load bearing and appear in several of these.
//!
//! `DONE WHEN` is how an objective arrives with something checkable attached. A task refuses to
//! exist without at least one condition, so an assigner that writes only prose has not finished
//! assigning, and asking again is cheaper than accepting work nobody can judge.
//!
//! `ACCEPT` or `REJECT` as the first word of a review is how a decision is read back out. A
//! review that buries its verdict in a paragraph is a review nothing downstream can act on.

/// Asked of anybody handing work down, so the task has something checkable on it.
const CONDITIONS: &str = "\
Then, on a line of its own, write DONE WHEN and under it one line per thing that must be true \
before this counts as finished. Each one has to be something a person could check rather than \
an opinion. Write at least one.";

pub fn carl_hands_down(request: &str) -> String {
    format!(
        "JJ has asked for this:\n\n{request}\n\nHand Adrian, who leads the coding department, \
         one coding objective. Say what must be true when it is done and why JJ wants it. Do \
         not say how to build it, do not name files, and do not write any code.\n\n\
         {CONDITIONS}\n\nAnswer with the objective and nothing else. No preamble."
    )
}

pub fn adrian_hands_down(objective: &str) -> String {
    format!(
        "Carl has given you this objective:\n\n{objective}\n\nThis is Factorio work, so it \
         belongs to Mason, who leads the Factorio sub department. Write the objective for \
         Mason. Say what must be true when it is done and any constraint he has to respect. \
         Leave how it is done to him.\n\n{CONDITIONS}\n\nAnswer with the objective and nothing \
         else. No preamble."
    )
}

pub fn mason_writes_task(objective: &str, workdir: &str) -> String {
    format!(
        "Adrian has given you this objective:\n\n{objective}\n\nYou and Nora are working in \
         {workdir}. Turn this into exactly one concrete task for Nora. Give her the context she \
         needs so she does not have to guess, and say what verification she must run. She owns \
         how it is implemented, so do not write the implementation for her.\n\n{CONDITIONS}\n\n\
         Answer with the task and nothing else. No preamble."
    )
}

/// Asked again when an agent handed work down without anything checkable attached.
///
/// One retry rather than a fallback condition invented here. A generic "it works" would let
/// the task exist while defeating the reason verification is required at all.
pub fn conditions_again(said: &str) -> String {
    format!(
        "You wrote this:\n\n{said}\n\nIt has no DONE WHEN section, so there is nothing anybody \
         can check it against and the task cannot be created. Write it again, the same \
         objective, with DONE WHEN on a line of its own and one checkable condition per line \
         under it."
    )
}

pub fn nora_first(task: &str, must: &[String], workdir: &str) -> String {
    format!(
        "Mason has given you this task:\n\n{task}\n\nIt is done when all of these are true:\n\
         {}\n\nYou are working in {workdir}. Do the work now. Use your tools to read and change \
         the files and to run commands. Run the verification and read what it actually \
         printed.\n\nThen report back using the four headings DID, VERIFIED, FOUND and BLOCKED, \
         each on its own line. Put the word none under BLOCKED if nothing is blocking you.",
        bullets(must)
    )
}

pub fn nora_again(why: &str, attempts: u32, limit: u32) -> String {
    format!(
        "Mason has reviewed that and asked for changes. He said:\n\n{why}\n\nFix it. Read what \
         he pointed at before you change anything, and rerun the verification when you are \
         done. You have submitted this task {attempts} time(s) and the limit is {limit}, after \
         which it stops being yours.\n\nReport back in the same four heading shape."
    )
}

pub fn mason_reviews(summary: &str, must: &[String]) -> String {
    format!(
        "Nora has submitted the task you gave her:\n\n{summary}\n\nIt is done only when all of \
         these are true:\n{}\n\nReview it yourself. Read the files she says she changed and \
         rerun the verification rather than believing her summary. If you cannot confirm \
         something, that is a reject.\n\nAnswer with ACCEPT or REJECT as the very first word on \
         the first line, then the reason on the next line. If you reject, say exactly what she \
         must fix.",
        bullets(must)
    )
}

/// The report Mason sends up, either way.
///
/// A task he gave up on carries the whole history rather than the last complaint, because
/// Adrian is being asked to decide something Mason could not, and a verdict with no case
/// behind it can only be rubber stamped.
pub fn mason_reports_up(accepted: bool, attempts: u32, last: &str, history: &str) -> String {
    let how = if accepted {
        format!("You accepted it after {attempts} attempt(s). Your reason was: {last}")
    } else {
        format!(
            "You rejected it {attempts} times, which is the limit, so it is no longer Nora's \
             and it goes to Adrian to decide. Here is the whole history of the task and every \
             review:\n\n{history}"
        )
    };
    format!(
        "That task is as far as your sub department can take it. {how}\n\nReport up to Adrian. \
         Say what was asked, what Nora actually did, what you verified yourself rather than \
         took on trust, and anything he needs to decide. Be brief.\n\nAnswer with the report \
         and nothing else."
    )
}

pub fn adrian_reports_up(accepted: bool, from_mason: &str) -> String {
    let ask = if accepted {
        "Report up to Carl. Say what the department delivered and whether it was verified."
    } else {
        "Mason could not accept this and it has come to you. Decide what happens next and say \
         why. Then report up to Carl, including your decision."
    };
    format!(
        "Mason reports:\n\n{from_mason}\n\n{ask} Be brief.\n\nAnswer with the report and \
         nothing else."
    )
}

pub fn carl_answers_jj(request: &str, from_adrian: &str) -> String {
    format!(
        "JJ asked for this:\n\n{request}\n\nAdrian reports:\n\n{from_adrian}\n\nTell JJ what \
         happened, in two or three sentences. Say plainly whether he got what he asked for. Do \
         not restate the work and do not add anything the report did not say."
    )
}

fn bullets(must: &[String]) -> String {
    must.iter()
        .map(|m| format!("  - {m}"))
        .collect::<Vec<_>>()
        .join("\n")
}
