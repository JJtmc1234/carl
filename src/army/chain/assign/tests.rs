//! What a lead may and may not do with the work it holds.

use super::*;
use crate::army::task::Verification;

fn verification() -> Verification {
    Verification::of(["the tests pass"]).unwrap()
}

/// The task a lead is holding, handed down by Carl.
fn held_by(lead: &str) -> Task {
    Task::assign("carl", lead, "get the issues under control", verification()).unwrap()
}

fn chose(agent: &str) -> HandedOn {
    HandedOn {
        agent: agent.into(),
        goal: "triage the open issues and say which are real".into(),
        must: vec!["every issue named has a file and a line".into()],
    }
}

#[test]
fn a_well_formed_answer_is_read_into_its_parts() {
    let said = "AGENT:\niris\n\nTASK:\nSweep the repositories and file what is actually wrong.\n\n\
                DONE WHEN:\n- every issue names a file and a line\n- nothing already open is refiled\n";
    let got = read_choice(said);

    assert_eq!(got.agent, "iris");
    assert_eq!(
        got.goal,
        "Sweep the repositories and file what is actually wrong."
    );
    assert_eq!(got.must.len(), 2);
}

/// A lead that explains itself has still named somebody.
#[test]
fn a_name_with_reasoning_after_it_is_still_a_name() {
    let said = "AGENT:\nEvan, because this is a fix rather than a report\n\nTASK:\ndo it\n\n\
                DONE WHEN:\n- it is done\n";
    assert_eq!(read_choice(said).agent, "evan");
}

#[test]
fn a_lead_may_hand_work_to_its_own_agent() {
    let parent = held_by("adrian");
    let task = hand_on("adrian", &parent, &chose("iris")).unwrap();

    assert_eq!(task.owner, "iris");
    assert_eq!(task.created_by, "adrian");
    assert_eq!(
        task.parent.as_ref(),
        Some(&parent.id),
        "the line back up has to stay walkable, or the subtask is an orphan nobody reviews"
    );
}

/// The whole point. A lead that could reach past its own people is not a lead.
#[test]
fn a_lead_reaching_for_somebody_elses_agent_is_refused() {
    for (lead, theirs) in [
        ("adrian", "nora"),
        ("adrian", "miles"),
        ("mason", "iris"),
        ("mason", "evan"),
        ("olivia", "nora"),
        ("olivia", "iris"),
    ] {
        let parent = held_by(lead);
        let err = hand_on(lead, &parent, &chose(theirs))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot hand work to"),
            "{lead} to {theirs}: {err}"
        );
    }
}

/// And a lead cannot hand work upward or sideways to another lead.
#[test]
fn a_lead_cannot_hand_work_to_carl_or_to_another_lead() {
    for to in ["carl", "mason", "olivia", "jj"] {
        let parent = held_by("adrian");
        assert!(
            hand_on("adrian", &parent, &chose(to)).is_err(),
            "adrian was allowed to hand work to {to}"
        );
    }
}

#[test]
fn naming_nobody_is_refused_and_says_why() {
    let parent = held_by("adrian");
    let err = hand_on("adrian", &parent, &chose(""))
        .unwrap_err()
        .to_string();
    assert!(err.contains("named nobody"), "{err}");
}

#[test]
fn an_empty_task_is_refused() {
    let parent = held_by("adrian");
    let mut chosen = chose("iris");
    chosen.goal = "  ".into();
    let err = hand_on("adrian", &parent, &chosen).unwrap_err().to_string();
    assert!(err.contains("empty task"), "{err}");
}

/// Every lead is offered exactly its own people and nobody else's.
#[test]
fn the_question_offers_only_that_leads_agents() {
    let asked = ask_which_agent("adrian", "get the issues under control").unwrap();
    for mine in ["iris", "evan"] {
        assert!(asked.contains(mine), "{mine} was not offered:\n{asked}");
    }
    for theirs in ["nora", "miles"] {
        assert!(
            !asked.contains(&format!("  {theirs} -")),
            "{theirs} was offered to adrian and is not his"
        );
    }
}

/// Serena and Rowan lead nobody, and being asked to hand work down is a refusal rather than a
/// question with no answers in it.
#[test]
fn a_lead_with_nobody_under_them_says_so_rather_than_offering_an_empty_list() {
    for empty in ["serena", "rowan"] {
        let err = ask_which_agent(empty, "look into something")
            .unwrap_err()
            .to_string();
        assert!(err.contains("nobody to hand work to"), "{empty}: {err}");
    }
}
