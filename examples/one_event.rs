fn main() {
    let home = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let goal = std::env::args().nth(2).unwrap();
    let people = carl::army::personnel::Personnel::open(&home).unwrap();
    let t = carl::army::task::Task::assign(
        "mason",
        "nora",
        goal,
        carl::army::task::Verification::of(["cargo test passes"]).unwrap(),
    )
    .unwrap();
    let mut j = carl::army::event::Journal::open(people.journal_path()).unwrap();
    let r = j
        .append(
            "mason",
            carl::army::event::Event::Delegated {
                task: t.id.clone(),
                to: "nora".into(),
                goal: t.goal.clone(),
                parent: None,
                must: t.verification.must.clone(),
                project: None,
            },
        )
        .unwrap();
    println!("{}", r.seq);
}
