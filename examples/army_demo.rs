//! One real objective through three real Claude sessions, once, and then everything stops.
//!
//! Never run by `cargo test`. An example rather than a test on purpose: a model is slow, costs
//! money and answers differently every time, and a suite that needed one would be a suite nobody
//! could run offline. Everything this demonstrates is already proved deterministically in
//! `tests/vertical_slice.rs`. What this adds is the one thing a stand in cannot: that real Claude
//! sessions survive being supervised, resumed and spoken to through the same machinery.
//!
//! ```sh
//! cargo run --example army_demo -- /path/to/a/temporary/home
//! ```
//!
//! **Never point it at ~/.carl.** It founds an army, starts processes and writes a journal. It
//! refuses a home that already has anything in it, and it refuses the real one by name.
//!
//! Three conversations: Carl, Mason and Nora. Adrian's handover is done by this program without
//! a model, because Carl cannot hand work straight to a sub department lead and three
//! conversations is the bound this demo was given. Four processes are started, because starting
//! every enlisted agent is what a supervisor does, and the fourth is never spoken to, so it
//! costs nothing.
//!
//! One message per agent per step. No loops, no agent talking to another agent directly, and
//! nothing that could go round twice.

use std::path::{Path, PathBuf};
use std::time::Duration;

use carl::army::board::Board;
use carl::army::event::{Because, Event, Journal};
use carl::army::personnel::{Personnel, found};
use carl::army::runtime::Supervisor;
use carl::army::task::{Status, Task, Verification};

/// How long any one agent gets to answer.
const PATIENCE: Duration = Duration::from_secs(180);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home: PathBuf = std::env::args()
        .nth(1)
        .ok_or("give me a temporary home to work in")?
        .into();

    refuse_anything_but_a_fresh_temporary_home(&home)?;
    std::fs::create_dir_all(&home)?;

    let now = 1_000;
    found(&home, now)?;
    let people = Personnel::open(&home)?;
    let mut board = Board::open(&home)?;
    let mut supervisor = Supervisor::take(&home, "claude")?;

    let id = |name: &str| people.identity(name).expect("founded").id.clone();

    println!("starting every enlisted agent");
    let tick = supervisor.tick(&people, now)?;
    for line in tick.lines() {
        println!("{line}");
    }

    // Carl states the objective. He has no tools at all, by rank, so he cannot do anything about
    // it himself even if he decides he would like to.
    let objective_text = ask(
        &mut supervisor,
        &id("carl"),
        "carl",
        "JJ wants one thing: a short note written down saying that the army did what it was \
         asked. State the objective for the coding department in one sentence. Do not say how \
         it should be done and do not write anything yourself.",
    )?;

    let objective = Task::assign(
        "carl",
        "adrian",
        first_line(&objective_text),
        Verification::of(["a note exists that the department lead has read"])?,
    )?;
    board.delegate("carl", &objective)?;
    board.advance("adrian", &objective.id, Status::InHand)?;

    // Adrian's step, without a model. Three conversations is the bound.
    let departmental = Task::split_from(
        &objective,
        "adrian",
        "mason",
        first_line(&objective_text),
        Verification::of(["the worker produced the note and mason checked it himself"])?,
    )?;
    board.delegate("adrian", &departmental)?;
    board.advance("mason", &departmental.id, Status::InHand)?;

    // The workspace the worker is allowed to write in. The grant is recorded here. What would
    // enforce it is the capability server, in another process, which this demo does not run.
    let workspace = people.folder("nora").join("work");
    std::fs::create_dir_all(&workspace)?;
    let note = workspace.join("note.md");

    let written = ask(
        &mut supervisor,
        &id("mason"),
        "mason",
        &format!(
            "Your department objective is: {}\n\nNora's workspace is {}. Write one concrete \
             task for her, in one sentence, that produces a markdown file at {}. Then on a \
             second line, starting with MUST, say the one thing that has to be true for it to \
             be done. Two lines, nothing else.",
            first_line(&objective_text),
            workspace.display(),
            note.display()
        ),
    )?;

    let (goal, must) = two_lines(&written);
    let concrete = Task::split_from(
        &departmental,
        "mason",
        "nora",
        &goal,
        Verification::of([&must])?,
    )?
    .in_workspace(workspace.display().to_string());
    board.delegate("mason", &concrete)?;
    board.grant(
        "mason",
        &concrete.id,
        &format!("write under {}", workspace.display()),
    )?;

    // She has been up since the first tick, so this asks and finds there is nothing to do. Asked
    // anyway, because the caller does not get to assume: whether an agent has a process is the
    // supervisor's to answer and not Mason's to guess.
    let woken = supervisor.wake(
        &id("nora"),
        Because::Task {
            task: concrete.id.clone(),
        },
        now + 1,
    )?;
    println!(
        "\nasked for nora: {}",
        if woken { "woken" } else { "already up" }
    );
    supervisor.tick(&people, now + 2)?;
    board.advance("nora", &concrete.id, Status::InHand)?;

    let reported = ask(
        &mut supervisor,
        &id("nora"),
        "nora",
        &format!(
            "Your task: {goal}\n\nIt is done when: {must}\n\nYou may write only under {}. Do it \
             now, then reply with one line saying what you did.",
            workspace.display()
        ),
    )?;
    board.submit("nora", &concrete.id, reported.len())?;

    // Mason checks rather than trusts. He is handed what is actually on disk, not her account
    // of it, which is the whole difference between a review and a rubber stamp.
    let on_disk = std::fs::read_to_string(&note).unwrap_or_else(|e| format!("<unreadable: {e}>"));
    println!(
        "\n--- what is actually at {} ---\n{on_disk}---\n",
        note.display()
    );

    let verdict = ask(
        &mut supervisor,
        &id("mason"),
        "mason",
        &format!(
            "Nora reports: {}\n\nThis is what is actually in {}:\n{on_disk}\n\nIt is done when: \
             {must}\n\nReply with ACCEPT or REJECT as the first word, then one line saying why.",
            first_line(&reported),
            note.display()
        ),
    )?;

    let accepted = verdict.trim_start().to_uppercase().starts_with("ACCEPT");
    board.review("mason", &concrete.id, accepted, &first_line(&verdict))?;

    // Back up the chain, one level at a time.
    board.submit("mason", &departmental.id, verdict.len())?;
    board.review(
        "adrian",
        &departmental.id,
        accepted,
        "the worker's file was checked",
    )?;
    board.submit("adrian", &objective.id, verdict.len())?;
    board.review(
        "carl",
        &objective.id,
        accepted,
        "the department reported back",
    )?;

    let answer = ask(
        &mut supervisor,
        &id("carl"),
        "carl",
        &format!(
            "The coding department reports: {}\n\nIn one sentence, tell JJ whether what he asked \
             for happened.",
            first_line(&verdict)
        ),
    )?;

    Journal::open(home.join("run").join("events.jsonl"))?.append(
        "carl",
        Event::Decided {
            task: Some(objective.id.clone()),
            what: first_line(&answer),
        },
    )?;

    // Everything stops. Dropping the supervisor closes every session it is holding, which ends
    // every process it started.
    println!("\nstopping {} processes", supervisor.holding());
    drop(supervisor);

    println!("\n--- the record ---");
    for record in board.records()? {
        println!(
            "  {:>3} {:8} {}",
            record.seq,
            record.actor,
            record.event.kind()
        );
    }
    println!(
        "\nobjective: {}",
        board
            .get(&objective.id)?
            .map_or("gone".into(), |t| t.status.to_string())
    );
    println!("note: {}", note.display());
    Ok(())
}

/// One message to one agent, through the supervisor that holds its pipe.
fn ask(
    supervisor: &mut Supervisor,
    agent: &carl::army::personnel::AgentId,
    name: &str,
    text: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    println!("\n--- to {name} ---\n{text}");
    let said = supervisor.deliver(agent, text, PATIENCE)?;
    println!("--- {name} said ---\n{}", said.trim());
    Ok(said)
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("nothing")
        .to_string()
}

/// A goal and the condition under it, from an answer that was asked for in two lines.
fn two_lines(text: &str) -> (String, String) {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let goal = lines
        .first()
        .copied()
        .unwrap_or("write the note")
        .to_string();
    let must = lines
        .iter()
        .find_map(|l| {
            l.strip_prefix("MUST")
                .map(|rest| rest.trim_start_matches([':', ' ']).to_string())
        })
        .or_else(|| lines.get(1).map(|l| l.to_string()))
        .unwrap_or_else(|| "the file exists".to_string());
    (goal, must)
}

/// Refuses anything that is not an empty directory nobody is using.
///
/// The real home holds a live army, a journal and every conversation Carl has ever had. Founding
/// over it would be unrecoverable, and this program founds.
fn refuse_anything_but_a_fresh_temporary_home(home: &Path) -> Result<(), String> {
    let real = std::env::var("HOME").map(|h| PathBuf::from(h).join(".carl"));
    if real.map(|r| home == r).unwrap_or(false) {
        return Err("that is the real home. Give me a temporary directory.".into());
    }
    if home.join("run").exists() || home.join("army").exists() {
        return Err(format!(
            "{} already holds an army. Give me an empty directory.",
            home.display()
        ));
    }
    Ok(())
}
