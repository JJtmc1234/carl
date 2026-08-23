//! A home with a few finished objectives in it, so `carl army metrics` has something to read.
//!
//! For looking at the report against a real journal rather than only in tests. Every line goes
//! through the same `Board` the army uses, so the record this leaves behind is the shape the
//! real one has and not a fixture that happens to parse.
//!
//! ```sh
//! cargo run --example seed_objectives /tmp/demo
//! ./target/debug/carl --home /tmp/demo army metrics
//! ```

use carl::army::event::{Event, Intervention};
use carl::army::task::{Status, Task, Verification};
use carl::army::{Board, Journal};

fn must(what: &str) -> Verification {
    Verification::of([what]).unwrap()
}

fn main() {
    let home = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("give it a directory to build the home in"),
    );
    let people = carl::army::personnel::found(&home, 1).unwrap();
    let path = people.journal_path().to_path_buf();
    let mut board = Board::at(&path).unwrap();
    let mut journal = Journal::open(&path).unwrap();

    // Three objectives. One clean, one JJ had to step into, one still open, which is roughly
    // what a real week looks like and is the only mix the report is interesting on.
    for (n, goal) in [
        "make JJtorio start faster",
        "get the panel onto the BOOX",
        "work out why the supervisor restarts at night",
    ]
    .into_iter()
    .enumerate()
    {
        journal
            .append(
                "jj",
                Event::Intervened {
                    what: Intervention::Objective { what: goal.into() },
                },
            )
            .unwrap();

        let objective = Task::assign("jj", "carl", goal, must("JJ says it is done")).unwrap();
        board.delegate("jj", &objective).unwrap();
        board
            .advance("carl", &objective.id, Status::InHand)
            .unwrap();

        let concrete = Task::split_from(
            &objective,
            "carl",
            "mason",
            "the part somebody can actually do",
            must("there is a test"),
        )
        .unwrap();
        board.delegate("carl", &concrete).unwrap();
        board
            .advance("mason", &concrete.id, Status::InHand)
            .unwrap();

        // The third one is where it stops. An objective still in flight is the ordinary case
        // and the report has to be able to show one without counting it as a failure.
        if n == 2 {
            journal
                .append(
                    "jj",
                    Event::Intervened {
                        what: Intervention::Stopped {
                            task: concrete.id.clone(),
                            why: "look at the unit file first".into(),
                        },
                    },
                )
                .unwrap();
            continue;
        }

        // The second one gets sent back once, so there is a rejection and a retry to see.
        if n == 1 {
            board.submit("mason", &concrete.id, 20).unwrap();
            board
                .review("carl", &concrete.id, false, "no test with it")
                .unwrap();
            board
                .advance("mason", &concrete.id, Status::InHand)
                .unwrap();
            journal
                .append(
                    "jj",
                    Event::Intervened {
                        what: Intervention::Message {
                            to: "mason".into(),
                            what: "the test goes next to the fix".into(),
                        },
                    },
                )
                .unwrap();
        }

        board.submit("mason", &concrete.id, 40).unwrap();
        board
            .review(
                "carl",
                &concrete.id,
                true,
                "read it, the test fails without it",
            )
            .unwrap();
        board.submit("carl", &objective.id, 40).unwrap();
        board
            .review("jj", &objective.id, true, "that is what I asked for")
            .unwrap();
    }

    // A refusal and a crash that came back, so the two halves nobody writes on purpose are
    // represented as well.
    journal
        .append(
            "carl",
            Event::Refused {
                what: "assign work to nora".into(),
                why: "carl cannot hand work straight to nora, ask mason".into(),
            },
        )
        .unwrap();

    let nora = people.identity("nora").unwrap().id.clone();
    journal
        .append(
            "supervisor",
            Event::AgentCrashed {
                agent: nora.clone(),
                name: "nora".into(),
                code: Some(1),
                attempt: 1,
            },
        )
        .unwrap();
    journal
        .append(
            "supervisor",
            Event::AgentStarted {
                agent: nora,
                name: "nora".into(),
                continuity: carl::army::runtime::Continuity {
                    process: carl::army::runtime::Process::Replaced,
                    session: carl::army::runtime::Session::Resumed,
                    memory: carl::army::runtime::Memory::Kept,
                },
                attempt: 1,
            },
        )
        .unwrap();

    println!("{}", home.display());
}
