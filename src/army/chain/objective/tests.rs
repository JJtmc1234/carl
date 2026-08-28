//! What Carl may and may not do with an objective.

use super::*;

fn journal() -> (tempfile::TempDir, Journal) {
    let dir = tempfile::tempdir().unwrap();
    let j = Journal::open(dir.path().join("events.jsonl")).unwrap();
    (dir, j)
}

fn chose(lead: &str) -> HandedDown {
    HandedDown {
        lead: lead.into(),
        goal: "make the belt planner faster".into(),
        must: vec!["the throughput test passes".into()],
    }
}

#[test]
fn a_well_formed_answer_is_read_into_its_parts() {
    let said = "LEAD:\nmason\n\nOBJECTIVE:\nMake JJtorio start faster.\n\nDONE WHEN:\n\
                - the load time is under two seconds\n- the existing tests still pass\n";
    let got = read_choice(said);

    assert_eq!(got.lead, "mason");
    assert_eq!(got.goal, "Make JJtorio start faster.");
    assert_eq!(
        got.must,
        vec![
            "the load time is under two seconds".to_string(),
            "the existing tests still pass".to_string()
        ]
    );
}

/// A model that explains itself has still named somebody, and refusing that would be refusing a
/// right answer over punctuation.
#[test]
fn a_name_with_reasoning_after_it_is_still_a_name() {
    let said = "LEAD:\nMason, because this is Factorio work\n\nOBJECTIVE:\ndo the thing\n\n\
                DONE WHEN:\n- it is done\n";
    assert_eq!(read_choice(said).lead, "mason");
}

#[test]
fn an_answer_with_no_headings_names_nobody_rather_than_guessing() {
    let got = read_choice("I think Mason should probably take this one.");
    assert_eq!(got.lead, "", "nothing was in a LEAD section");
}

#[test]
fn carl_may_hand_an_objective_to_a_real_lead() {
    let (_d, mut j) = journal();
    let (record, task) = hand_down(&mut j, 15, &chose("mason")).unwrap();

    assert_eq!(record.actor, "carl");
    assert_eq!(task.owner, "mason");
    assert_eq!(task.created_by, "carl");
    assert_eq!(
        task.objective,
        Some(15),
        "the task knows which objective it answers"
    );

    let crate::army::event::Event::Delegated { objective, to, .. } = &record.event else {
        panic!("the record is not a delegation: {:?}", record.event);
    };
    assert_eq!(*objective, Some(15), "and so does the record");
    assert_eq!(to, "mason");
}

/// The whole point. A lead Carl could talk his way past is not a lead.
#[test]
fn carl_naming_a_worker_is_refused_rather_than_obeyed() {
    for worker in ["nora", "iris", "evan", "miles"] {
        let (dir, mut j) = journal();
        let err = hand_down(&mut j, 15, &chose(worker))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("cannot hand an objective to"),
            "{worker}: {err}"
        );
        assert!(err.contains("His leads are"), "and it says who he may use");

        let written = crate::army::event::read(dir.path().join("events.jsonl")).unwrap();
        assert!(
            written.is_empty(),
            "{worker} was refused and nothing was written"
        );
    }
}

#[test]
fn a_name_nobody_holds_is_refused() {
    let (_d, mut j) = journal();
    let err = hand_down(&mut j, 15, &chose("gandalf"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("cannot hand an objective to gandalf"), "{err}");
}

#[test]
fn naming_nobody_at_all_is_refused_and_says_why() {
    let (_d, mut j) = journal();
    let err = hand_down(&mut j, 15, &chose("")).unwrap_err().to_string();
    assert!(err.contains("named nobody"), "{err}");
}

#[test]
fn an_objective_with_no_words_in_it_is_refused() {
    let (_d, mut j) = journal();
    let mut chosen = chose("mason");
    chosen.goal = "   ".into();
    let err = hand_down(&mut j, 15, &chosen).unwrap_err().to_string();
    assert!(err.contains("came back empty"), "{err}");
}

/// Every lead is offered, and no worker is, so Carl cannot pick somebody he may not have.
#[test]
fn the_question_offers_every_lead_and_no_worker() {
    let asked = ask_which_lead("expand the agent fleet");

    for lead in ["adrian", "mason", "olivia", "serena", "rowan"] {
        assert!(asked.contains(lead), "{lead} was not offered:\n{asked}");
    }
    for worker in ["nora", "iris", "evan", "miles"] {
        assert!(
            !asked.contains(&format!("  {worker} -")),
            "{worker} was offered and is not Carl's to hand work to"
        );
    }
    assert!(
        asked.contains("expand the agent fleet"),
        "and it carries it"
    );
}

/// A department added to the table has to be offered without anybody editing this file.
#[test]
fn the_leads_offered_come_from_the_table() {
    let asked = ask_which_lead("anything");
    for lead in org::reports_of("carl") {
        assert!(asked.contains(lead.name), "{} is missing", lead.name);
    }
}
