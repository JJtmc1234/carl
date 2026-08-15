//! One request, all the way down the chain and all the way back.
//!
//! Every step is a real `Task`, so the rules in `org.rs` and `task.rs` are the only route the
//! work can take rather than a description of the route it usually takes. Carl cannot skip
//! Adrian here because `Task::assign` refuses it, not because this file remembers not to.
//!
//! The one decision that is genuinely this module's is when to stop. Nora gets her attempt and
//! two corrections. After the third rejection the task is abandoned rather than tried again,
//! and it goes to Adrian, because a task rejected three times is usually a task that was
//! written wrong rather than done wrong.

use super::words;
use super::{Chain, MAX_ATTEMPTS, Next, Passage, after_review, read_handback, read_verdict};
use crate::Result;
use crate::army::task::{Status, Task};

/// JJ to Carl to Adrian to Mason to Nora, and back up again.
pub fn campaign(chain: &mut Chain, request: &str) -> Result<Passage> {
    let workdir = chain.workdir().display().to_string();

    // Down. Each handover creates a real task, and each one is refused if it does not follow
    // the chain or arrives with nothing that can be checked.
    let said = chain.ask("carl", &words::carl_hands_down(request))?;
    let (mut for_adrian, adrian_goal, _) = chain.hand_down("carl", "adrian", &said, None)?;
    chain.advance(&mut for_adrian, "adrian", Status::InHand)?;

    let said = chain.ask("adrian", &words::adrian_hands_down(&adrian_goal))?;
    let (mut for_mason, mason_goal, _) =
        chain.hand_down("adrian", "mason", &said, Some(&for_adrian))?;
    chain.advance(&mut for_mason, "mason", Status::InHand)?;

    let said = chain.ask("mason", &words::mason_writes_task(&mason_goal, &workdir))?;
    let (mut for_nora, nora_goal, must) =
        chain.hand_down("mason", "nora", &said, Some(&for_mason))?;

    let done = work_and_review(chain, &mut for_nora, &nora_goal, &must, &workdir)?;

    // Up. Mason reports whatever happened, and it climbs one step at a time. Nobody skips a
    // level going up either, because each task is only reviewable by whoever created it.
    chain.advance(&mut for_mason, "mason", Status::Submitted)?;
    let from_mason = chain.ask(
        "mason",
        &words::mason_reports_up(done.accepted, done.attempts, &done.why, &done.history),
    )?;
    settle(chain, &mut for_mason, "adrian", done.accepted, &from_mason)?;

    chain.advance(&mut for_adrian, "adrian", Status::Submitted)?;
    let from_adrian = chain.ask(
        "adrian",
        &words::adrian_reports_up(done.accepted, &from_mason),
    )?;
    settle(chain, &mut for_adrian, "carl", done.accepted, &from_adrian)?;

    let answer = chain.ask("carl", &words::carl_answers_jj(request, &from_adrian))?;
    chain.decided("carl", Some(&for_adrian.id), &answer)?;

    Ok(Passage {
        request: request.to_string(),
        for_adrian: adrian_goal,
        for_mason: mason_goal,
        for_nora: nora_goal,
        tasks: vec![for_adrian, for_mason, for_nora],
        attempts: done.attempts,
        accepted: done.accepted,
        answer,
    })
}

/// What came of the one task that was actually worked on.
struct Done {
    accepted: bool,
    attempts: u32,
    /// The reviewer's last word, either why he accepted or why he could not.
    why: String,
    /// Every attempt and every review, for the report that goes up.
    history: String,
}

/// Nora works, Mason reviews, and it goes round until it is accepted or runs out of attempts.
fn work_and_review(
    chain: &mut Chain,
    task: &mut Task,
    goal: &str,
    must: &[String],
    workdir: &str,
) -> Result<Done> {
    let mut history = String::new();
    let mut changes: Option<String> = None;

    loop {
        chain.advance(task, "nora", Status::InHand)?;

        let prompt = match &changes {
            None => words::nora_first(goal, must, workdir),
            Some(why) => words::nora_again(why, task.attempts, MAX_ATTEMPTS),
        };
        let said = chain.ask("nora", &prompt)?;
        let handback = read_handback(&said);

        // Submitted before reviewed, always. Nothing reaches accepted without having been
        // offered, and the attempt counter this loop gives up on lives on the submission.
        chain.advance(task, "nora", Status::Submitted)?;
        chain.submitted(&task.id, task.attempts, said.split_whitespace().count())?;

        let reviewed = chain.ask("mason", &words::mason_reviews(&handback.summary(), must))?;
        let (verdict, why) = read_verdict(&reviewed);
        chain.reviewed("mason", &task.id, verdict.accepted(), &why)?;

        history.push_str(&format!(
            "\nAttempt {}:\n{}\nMason {}: {why}\n",
            task.attempts,
            handback.summary(),
            if verdict.accepted() {
                "accepted"
            } else {
                "rejected"
            }
        ));

        // The retry rule itself lives in `after_review`, so what runs here is the same thing
        // the tests check rather than a second copy of it that can drift.
        match after_review(verdict.accepted(), task.attempts) {
            Next::Accept => {
                chain.advance(task, "mason", Status::Accepted)?;
                return Ok(Done {
                    accepted: true,
                    attempts: task.attempts,
                    why,
                    history,
                });
            }
            Next::GiveUp => {
                // Abandoned rather than left open, so the record does not show a task somebody
                // might still be waiting on, and it goes up with its whole history.
                chain.advance(task, "mason", Status::Abandoned)?;
                return Ok(Done {
                    accepted: false,
                    attempts: task.attempts,
                    why,
                    history,
                });
            }
            Next::Correct => {
                chain.advance(task, "mason", Status::ChangesRequested)?;
                changes = Some(why);
            }
        }
    }
}

/// A boss finishing with a task their report just came in on.
///
/// Accepted when the work below succeeded. Abandoned when it did not, because a department
/// whose only piece of work was given up on has not delivered, and recording that as accepted
/// would make the record say the opposite of what happened.
fn settle(
    chain: &mut Chain,
    task: &mut Task,
    boss: &str,
    accepted: bool,
    report: &str,
) -> Result<()> {
    chain.reviewed(boss, &task.id, accepted, report)?;
    let next = if accepted {
        Status::Accepted
    } else {
        Status::Abandoned
    };
    chain.advance(task, boss, next)
}
