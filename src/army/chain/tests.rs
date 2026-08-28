//! What the chain has to get right, checked without spending a model call on any of it.
//!
//! The real end to end run is the proof that four processes talk to each other. These are the
//! proof that the rules hold, and they have to be separate, because a rule that is only ever
//! exercised by a live campaign is a rule nobody checks when the campaign is slow.

use super::*;
use crate::army::org::{self, Rank};
use crate::army::task::{Status, Task, Verification};

fn verification() -> Verification {
    Verification::of(["cargo test passes"]).unwrap()
}

fn chain() -> (Chain, tempfile::TempDir) {
    let d = tempfile::tempdir().unwrap();
    // A program that does not exist. Nothing here asks a model anything, and pointing at a
    // real claude would make these tests cost money and take minutes.
    let c = Chain::new(
        "/nonexistent/definitely-not-claude",
        d.path(),
        d.path().join("events.jsonl"),
    )
    .unwrap();
    (c, d)
}

/// The shortcut that looks harmless once and removes the point of having two leads.
#[test]
fn work_only_ever_moves_one_step_down_the_chain() {
    assert!(Task::assign("carl", "adrian", "do it", verification()).is_ok());
    assert!(Task::assign("mason", "nora", "do it", verification()).is_ok());

    let err = Task::assign("carl", "nora", "do it", verification())
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot hand work straight to"), "{err}");
    assert!(err.contains("mason"), "and names the route: {err}");

    assert!(
        Task::assign("nora", "mason", "do it", verification()).is_err(),
        "a worker does not assign her lead"
    );
    assert!(
        Task::assign("adrian", "nora", "do it", verification()).is_err(),
        "a lead does not reach past his own sub department lead"
    );
}

/// Carl is the one agent who must never produce work, whatever a prompt talks him into.
///
/// Two independent guards, because either alone is one forgotten call away from nothing. The
/// check refuses him when something asks, and the empty tool list means a process that never
/// asks still has no way to write a file.
#[test]
fn carl_can_never_do_the_work_himself() {
    assert!(org::check_may_implement("carl", false).is_err());
    assert!(
        org::check_may_implement("carl", true).is_err(),
        "not even in an emergency"
    );

    let tools = tools_for(Rank::Chief);
    assert!(
        tools.is_empty(),
        "the chief runs with no tools at all: {tools:?}"
    );

    for forbidden in ["Write", "Edit", "Bash"] {
        assert!(
            !tools.iter().any(|t| t.contains(forbidden)),
            "carl must not be granted {forbidden}"
        );
    }

    // And his brief says the same thing, so the model is not fighting the harness.
    let brief = brief_for(org::find("carl").unwrap());
    assert!(brief.contains("never write"), "{brief}");
}

/// A lead reviews, so it needs to be able to rerun what it is reviewing, and it does not need
/// an editor to do that.
#[test]
fn a_lead_may_check_the_work_but_not_write_it() {
    let tools = tools_for(Rank::Lead);
    assert!(
        tools.iter().any(|t| t == "Bash"),
        "a reviewer must be able to rerun it"
    );
    assert!(
        !tools.iter().any(|t| t == "Write" || t == "Edit"),
        "{tools:?}"
    );

    let worker = tools_for(Rank::Worker);
    assert!(worker.iter().any(|t| t == "Write"));
    assert!(worker.iter().any(|t| t == "Edit"));
}

/// Nothing anywhere grants extra privileges, and there is nowhere to ask for them.
#[test]
fn no_tool_list_raises_privileges() {
    for rank in [Rank::Human, Rank::Chief, Rank::Lead, Rank::Worker] {
        for tool in tools_for(rank) {
            let t = tool.to_lowercase();
            assert!(!t.contains("sudo"), "{tool}");
            assert!(!t.contains("admin"), "{tool}");
        }
    }
}

/// A worker holding two jobs picks one and quietly drops the other, and the dropped one still
/// reads as assigned to anybody looking.
#[test]
fn nobody_holds_two_tasks_at_once() {
    let (mut c, _d) = chain();
    let first = Task::assign("mason", "nora", "the first job", verification()).unwrap();

    assert!(c.check_free("nora").is_ok(), "she starts with nothing");
    c.now_holding("nora", &first.id).unwrap();

    let err = c.check_free("nora").unwrap_err().to_string();
    assert!(err.contains("one task at a time"), "{err}");
    assert!(err.contains(first.id.as_str()), "and says which one: {err}");
}

/// Finishing frees her, and only finishing does.
#[test]
fn a_task_that_settles_releases_its_owner() {
    let (mut c, _d) = chain();
    let mut t = Task::assign("mason", "nora", "the job", verification()).unwrap();
    c.now_holding("nora", &t.id).unwrap();

    c.advance(&mut t, "nora", Status::InHand).unwrap();
    c.advance(&mut t, "nora", Status::Submitted).unwrap();
    assert!(
        c.check_free("nora").is_err(),
        "still hers while it is reviewed"
    );

    c.advance(&mut t, "mason", Status::ChangesRequested)
        .unwrap();
    assert!(
        c.check_free("nora").is_err(),
        "a rejection does not free her"
    );

    c.advance(&mut t, "nora", Status::InHand).unwrap();
    c.advance(&mut t, "nora", Status::Submitted).unwrap();
    c.advance(&mut t, "mason", Status::Accepted).unwrap();
    assert!(c.check_free("nora").is_ok(), "accepted is finished");
}

/// The single most tempting shortcut in the design.
#[test]
fn nora_cannot_accept_her_own_work() {
    let (mut c, _d) = chain();
    let mut t = Task::assign("mason", "nora", "the job", verification()).unwrap();
    c.advance(&mut t, "nora", Status::InHand).unwrap();
    c.advance(&mut t, "nora", Status::Submitted).unwrap();

    assert!(c.advance(&mut t, "nora", Status::Accepted).is_err());
    assert_eq!(t.status, Status::Submitted, "and nothing moved");
}

/// One attempt and two corrections, then it stops being hers. This is the rule JJ asked for,
/// on its own, so it can be checked without a model.
#[test]
fn a_third_rejection_gives_up_rather_than_trying_again() {
    assert_eq!(after_review(true, 1), Next::Accept);
    assert_eq!(after_review(false, 1), Next::Correct, "correction one");
    assert_eq!(after_review(false, 2), Next::Correct, "correction two");
    assert_eq!(
        after_review(false, 3),
        Next::GiveUp,
        "and no third correction"
    );
    assert_eq!(MAX_ATTEMPTS, 3);

    // Accepting on the last attempt is still accepting.
    assert_eq!(after_review(true, MAX_ATTEMPTS), Next::Accept);
}

/// The whole retry loop against the real task type, so the states it walks through are the
/// ones the shared rules allow.
#[test]
fn three_attempts_end_in_a_task_nobody_still_holds() {
    let (mut c, _d) = chain();
    let mut t = Task::assign("mason", "nora", "the job", verification()).unwrap();
    c.now_holding("nora", &t.id).unwrap();

    for attempt in 1..=MAX_ATTEMPTS {
        c.advance(&mut t, "nora", Status::InHand).unwrap();
        c.advance(&mut t, "nora", Status::Submitted).unwrap();
        assert_eq!(t.attempts, attempt);

        match after_review(false, t.attempts) {
            Next::Correct => c
                .advance(&mut t, "mason", Status::ChangesRequested)
                .unwrap(),
            Next::GiveUp => c.advance(&mut t, "mason", Status::Abandoned).unwrap(),
            Next::Accept => panic!("nothing was accepted"),
        }
    }

    assert_eq!(t.attempts, 3);
    assert_eq!(t.status, Status::Abandoned);
    assert!(t.settled());
    assert!(c.check_free("nora").is_ok(), "it is no longer hers");
}

/// A report has to come back in a shape a reviewer can act on.
#[test]
fn a_report_is_read_into_its_four_parts() {
    let said = "DID:\nrewrote the belt counter in control.lua\n\nVERIFIED:\n\
                ran the mod, the counter reads 42 and used to read 41\n\n\
                FOUND:\nthe old code counted the header slot\n\nBLOCKED:\nnone";
    let h = read_handback(said);

    assert!(h.did.contains("belt counter"));
    assert!(h.verified.contains("reads 42"));
    assert!(h.found.contains("header slot"));
    assert_eq!(h.blocked, None, "the word none is not a blocker");
    assert!(h.summary().contains("VERIFIED"));
}

#[test]
fn a_real_blocker_survives_and_a_missing_one_does_not() {
    let blocked = read_handback(
        "DID:\ntried\n\nVERIFIED:\nnothing ran\n\nBLOCKED:\n\
                                 factorio is not installed, so the mod cannot load",
    );
    assert!(blocked.blocked.unwrap().contains("not installed"));

    for none in ["none", "None.", "  no blockers  "] {
        let h = read_handback(&format!("DID:\nx\n\nVERIFIED:\ny\n\nBLOCKED:\n{none}"));
        assert_eq!(h.blocked, None, "{none:?}");
    }
}

/// Headings arrive with whatever markup the model felt like, and mean the same thing.
#[test]
fn the_markup_around_a_heading_does_not_matter() {
    let said = "## DID\nchanged it\n\n**VERIFIED:**\nran it\n\n- FOUND:\nnothing\n\nBLOCKED\nnone";
    let h = read_handback(said);
    assert_eq!(h.did, "changed it");
    assert_eq!(h.verified, "ran it");
}

/// A worker who ignores the shape has still done the work, and throwing it away would be
/// worse than handing the reviewer everything she said.
#[test]
fn an_answer_with_no_headings_is_still_carried() {
    let h = read_handback("I changed the lua file and ran the tests, all green.");
    assert!(h.did.contains("changed the lua file"));
    assert!(h.raw.contains("all green"));
}

#[test]
fn a_verdict_is_read_from_the_first_word() {
    let (v, why) = read_verdict("ACCEPT\nthe counter is right and the test proves it");
    assert_eq!(v, Verdict::Accept);
    assert!(why.contains("counter is right"));

    let (v, why) = read_verdict("REJECT: the test does not fail without the fix");
    assert_eq!(v, Verdict::Reject);
    assert!(why.contains("does not fail"), "{why}");
}

/// The one place this is deliberately unforgiving. A review with no decision in it must not
/// pass work nobody approved, which is the exact failure a review exists to prevent.
#[test]
fn a_review_that_does_not_decide_is_not_an_acceptance() {
    for said in [
        "I had a look and it seems broadly fine to me",
        "This is interesting work",
        "",
    ] {
        let (v, why) = read_verdict(said);
        assert_eq!(v, Verdict::Reject, "{said:?}");
        assert!(why.contains("no clear accept or reject"), "{why}");
    }
}

/// A later paragraph mentioning the other word must not flip a decision already made.
#[test]
fn only_the_first_decision_counts() {
    let (v, _) = read_verdict("ACCEPT\nI reject the idea that this needed a rewrite");
    assert_eq!(v, Verdict::Accept);
}

/// A task refuses to exist without something checkable, so an objective has to arrive with it.
#[test]
fn conditions_are_pulled_out_of_an_objective() {
    let said = "Make the belt counter correct.\n\nDONE WHEN\n- cargo test passes\n\
                - the new test fails without the fix";
    let (goal, must) = read_must(said);

    assert_eq!(must.len(), 2);
    assert_eq!(must[0], "cargo test passes");
    assert!(goal.contains("belt counter"));
    assert!(
        !goal.contains("DONE WHEN"),
        "the conditions leave the goal: {goal}"
    );
    assert!(Verification::of(must).is_ok());
}

#[test]
fn an_objective_with_no_conditions_yields_none() {
    let (goal, must) = read_must("Make it faster. It should be good.");
    assert!(must.is_empty());
    assert!(goal.contains("Make it faster"));
    assert!(
        Verification::of(must).is_err(),
        "and so no task can be made"
    );
}

/// Every brief has to carry the standing rules, because a brief that has to remember to
/// include them is a brief that will forget.
#[test]
fn every_brief_says_who_it_may_talk_to_and_keeps_the_house_style() {
    for agent in org::everyone() {
        let brief = brief_for(agent);
        assert!(brief.contains(agent.display), "{}", agent.name);
        assert!(
            brief.contains("chain of command"),
            "{} is missing the standing rules",
            agent.name
        );
        assert!(
            !brief.contains(';'),
            "{} breaks the house style",
            agent.name
        );

        match agent.reports_to {
            Some(boss) => assert!(
                brief.contains(boss),
                "{} is not told who to answer to",
                agent.name
            ),
            None => assert!(brief.contains("report to nobody"), "{}", agent.name),
        }
    }
}

/// A brief names exactly the agents it may hand work to, and separately names everybody.
///
/// This used to assert that Carl was never told Nora exists, on the grounds that an agent
/// cannot address somebody its own prompt never introduced. That protected the delegation rule
/// by keeping Carl ignorant, and ignorance turned out to have its own cost: asked how Miles was
/// getting on, Carl said he had never heard of him, when Miles is in the table working for
/// Olivia. So the chart is given to everybody and the delegation sentence stays exact.
#[test]
fn a_brief_names_the_agents_it_may_hand_work_to() {
    let carl = brief_for(org::find("carl").unwrap());

    // The sentence that grants delegation names the leads and nobody else.
    let grant = carl
        .lines()
        .find(|l| l.contains("only ones you may hand work to"))
        .expect("carl is not told who he may hand work to");
    for lead in ["adrian", "mason", "olivia", "serena", "rowan"] {
        assert!(
            grant.contains(lead),
            "{lead} is missing from the grant: {grant}"
        );
    }
    for worker in ["nora", "iris", "evan", "miles"] {
        assert!(
            !grant.contains(worker),
            "{worker} appears in the sentence that grants delegation: {grant}"
        );
    }

    // And the chart is there so he can say whose somebody is rather than denying they exist.
    assert!(carl.contains("nora"), "carl cannot name nora at all");
    assert!(carl.contains("miles"), "carl cannot name miles at all");

    let nora = brief_for(org::find("nora").unwrap());
    assert!(nora.contains("hand work to nobody"), "{nora}");
}

/// Everything that happened has to be answerable afterwards, refusals included.
#[test]
fn the_record_shows_what_was_stopped_as_well_as_what_happened() {
    let (mut c, d) = chain();
    let mut t = Task::assign("mason", "nora", "the job", verification()).unwrap();
    c.now_holding("nora", &t.id).unwrap();
    c.advance(&mut t, "nora", Status::InHand).unwrap();
    c.advance(&mut t, "nora", Status::Submitted).unwrap();

    // The shortcut, attempted and refused.
    assert!(c.advance(&mut t, "nora", Status::Accepted).is_err());
    assert!(c.check_free("nora").is_err());

    let records = crate::army::event::read(d.path().join("events.jsonl")).unwrap();
    let kinds: Vec<&str> = records.iter().map(|r| r.event.kind()).collect();
    assert!(kinds.contains(&"moved"), "{kinds:?}");
    assert_eq!(
        kinds.iter().filter(|k| **k == "refused").count(),
        2,
        "both refusals are written down: {kinds:?}"
    );
    assert!(records.iter().any(|r| r.actor == "nora"));
}

/// A reviewer who numbers or decorates the line means the same thing, and the reason after the
/// verdict must survive intact. Slicing from the word's length rather than from where it sits
/// cut the reason mid word, and with anything non ASCII on the line it could panic instead.
#[test]
fn the_reason_survives_whatever_sits_in_front_of_the_verdict() {
    for said in [
        "REJECT: the test does not fail without the fix",
        "1. REJECT the test does not fail without the fix",
        "**REJECT** the test does not fail without the fix",
        "- rejected. the test does not fail without the fix",
    ] {
        let (v, why) = read_verdict(said);
        assert_eq!(v, Verdict::Reject, "{said:?}");
        assert!(
            why.starts_with("the test does not fail"),
            "{said:?} gave {why:?}"
        );
    }
}

/// A review written with any non ASCII in it must be read rather than crash the run.
#[test]
fn a_verdict_line_with_non_ascii_does_not_panic() {
    let (v, why) = read_verdict("REJECT the ratio is wrong, 45 is not 40 either way");
    assert_eq!(v, Verdict::Reject);
    assert!(why.contains("ratio is wrong"), "{why}");

    let (v, _) = read_verdict("ACCEPT the numbers agree with the wiki");
    assert_eq!(v, Verdict::Accept);
}

/// JJ asked Carl how Miles was getting on and Carl said he had never heard of him.
///
/// That was true of what Carl had been told: the brief listed his direct reports and nothing
/// else. Miles is in the compiled table working for Olivia, so the right answer was "he is
/// Olivia's, ask her". An agent who cannot name the organisation dead ends every question about
/// somebody else's department.
#[test]
fn carl_can_name_everybody_even_the_agents_he_may_not_reach() {
    let carl = crate::army::org::require("carl").unwrap();
    let brief = brief_for(carl);

    for name in [
        "adrian", "mason", "olivia", "serena", "rowan", "iris", "evan", "nora", "miles",
    ] {
        assert!(brief.contains(name), "carl was never told {name} exists");
    }
    assert!(
        brief.contains("Olivia") || brief.contains("olivia"),
        "and who Miles belongs to"
    );
}

/// Handing somebody the whole chart must not turn into permission to use it.
#[test]
fn knowing_the_chart_is_not_permission_to_reach_across_it() {
    let carl = crate::army::org::require("carl").unwrap();
    let brief = brief_for(carl);
    assert!(
        brief.contains("Knowing this is not permission to use it"),
        "the chart was handed over without the rule that goes with it"
    );
    assert!(
        brief.contains("only to the agents"),
        "the delegation rule is not restated alongside the chart"
    );
}

/// A worker sees it too, so they can say who to ask rather than guessing.
#[test]
fn a_worker_is_told_the_organisation_as_well() {
    let miles = crate::army::org::require("miles").unwrap();
    let brief = brief_for(miles);
    assert!(
        brief.contains("olivia"),
        "miles was not told who he reports to"
    );
    assert!(brief.contains("carl"), "nor that Carl exists");
}
