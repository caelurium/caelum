use crate::{
    application::{Command, Event, Reply, Workspace},
    remote::*,
};

fn source() -> Source {
    Source {
        project: "/local/project".into(),
        repository: "example/project".into(),
        account: "owner".into(),
        base: "main".into(),
        environment: None,
    }
}
fn created(app: &mut Workspace) -> (String, u64, Plan) {
    app.remote.picker = Some(Picker::Ready(source()));
    let id = app
        .create_remote("main", "feat/new", Size::Small, 30)
        .unwrap();
    let (req, Command::Remote(Operation::Create(_, plan))) = app.commands().pop().unwrap() else {
        panic!()
    };
    (id, req, plan)
}
fn machine(id: &str) -> Machine {
    Machine {
        key: MachineId {
            host: Host::Boat,
            account: "owner".into(),
            id: id.into(),
        },
        name: "dev".into(),
        state: "idle".into(),
    }
}

#[test]
fn lost_create_reply_reuses_the_persisted_attempt_and_late_prepare_keeps_its_owner() {
    let mut app = Workspace::default();
    let (id, req, plan) = created(&mut app);
    app.apply(Event::Completed(req, Err("connection lost".into())));
    let mut restored = Workspace::default();
    restored.remote.saved = app.snapshot().remote;
    restored.select_remote(&id);
    let (req, Command::Remote(Operation::Create(_, retry))) = restored.commands().pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(retry, plan);
    restored.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Machine(machine("vm-a")))),
    ));
    let (prepare, Command::Remote(Operation::Prepare(target, base, Some(branch)))) =
        restored.commands().pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (target.machine.id.as_str(), base.as_str(), branch.as_str()),
        ("vm-a", "main", "feat/new")
    );
    let (other, _, _) = created(&mut restored);
    restored.apply(Event::Completed(
        prepare,
        Ok(Reply::Remote(Outcome::Prepared(machine("vm-a"), branch))),
    ));
    assert_eq!(restored.remote.saved.selected.as_ref(), Some(&other));
    assert!(matches!(
        restored.remote.saved.checkouts[&id].phase,
        Phase::Ready { .. }
    ));
    assert!(matches!(
        restored.remote.saved.checkouts[&other].phase,
        Phase::Create(_)
    ));
    assert!(restored.active_project.is_none());
    assert!(restored.sessions.is_empty());
}
