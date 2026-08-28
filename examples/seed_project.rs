fn main() {
    let home = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let people = carl::army::personnel::found(&home, 1).unwrap();

    let id = carl::ProjectId::new("jjtorio").unwrap();
    let projects = carl::providers::projects::Projects::open(&home);
    let mut p = carl::providers::projects::model::Project::new(
        id.clone(),
        "JJtorio",
        "make the mod start faster",
    );
    p.department = Some("factorio".into());
    p.phase = "first working version".into();
    projects.save(&p).unwrap();

    let t = carl::army::task::Task::assign(
        "mason",
        "nora",
        "cache the prototype lookup",
        carl::army::task::Verification::of(["cargo test passes"]).unwrap(),
    )
    .unwrap()
    .for_project(id);

    let mut j = carl::army::event::Journal::open(people.journal_path()).unwrap();
    j.append(
        "mason",
        carl::army::event::Event::Delegated {
            task: t.id.clone(),
            to: "nora".into(),
            goal: t.goal.clone(),
            parent: None,
            must: t.verification.must.clone(),
            project: t.project.clone(),

            workspace: t.workspace.clone(),
            objective: None,
        },
    )
    .unwrap();
    j.append(
        "nora",
        carl::army::event::Event::moved(
            &t.id,
            carl::army::task::Status::Assigned,
            carl::army::task::Status::InHand,
        ),
    )
    .unwrap();
    println!("project jjtorio, task {} linked to it, owner nora", t.id);
}
