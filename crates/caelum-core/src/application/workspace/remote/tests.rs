mod handoff;

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
        auto_stop_unix: None,
    }
}

#[test]
fn the_vm_folder_chooser_discovers_only_the_current_validated_project_without_starting_a_local_session(
) {
    use caelum_protocol::ProjectInfo;
    let mut app = Workspace::default();
    let choose = |app: &mut Workspace, path: &str| {
        app.remote_picker(path.into());
        app.commands()
            .into_iter()
            .find_map(|(id, c)| matches!(c,Command::Open(p) if p==path).then_some(id))
            .unwrap()
    };
    let old = choose(&mut app, "/alias/old");
    let new = choose(&mut app, "/alias/new");
    let resolved = |req, path: &str| {
        Event::Completed(
            req,
            Ok(Reply::Project(ProjectInfo {
                name: "project".into(),
                path: path.into(),
            })),
        )
    };
    app.apply(resolved(old, "/projects/old"));
    assert!(app.projects.is_empty() && app.commands().is_empty());
    app.apply(resolved(new, "/projects/new"));
    assert_eq!(app.projects.len(), 1);
    assert_eq!(app.projects[0].path, "/projects/new");
    assert!(
        matches!(app.commands().as_slice(),[(_,Command::Remote(Operation::Discover(p)))] if p=="/projects/new")
    );
    assert!(
        app.active_project.is_none()
            && app.sessions.is_empty()
            && app.remote.saved.checkouts.is_empty()
    );
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

#[test]
fn reopening_reattaches_to_the_saved_run_without_reposting_the_prompt() {
    let mut app = Workspace::default();
    let (id, req, _) = created(&mut app);
    let finish =
        |app: &mut Workspace, req, out| app.apply(Event::Completed(req, Ok(Reply::Remote(out))));
    finish(&mut app, req, Outcome::Machine(machine("vm-a")));
    let (req, _) = app.commands().pop().unwrap();
    finish(
        &mut app,
        req,
        Outcome::Prepared(machine("vm-a"), "feat/new".into()),
    );
    assert!(app.send_remote(&id, "Edit a file"));
    let saved_before_ack = app.snapshot();
    let (req, Command::Remote(Operation::Prompt(_, None, _))) = app.commands().pop().unwrap()
    else {
        panic!()
    };
    let run = Run {
        conversation: "c".into(),
        id: "r".into(),
        prompt: "p".into(),
    };
    finish(&mut app, req, Outcome::Accepted(run.clone()));
    let (req, _) = app.commands().pop().unwrap();
    finish(
        &mut app,
        req,
        Outcome::Page(Page {
            cursor: Some("after".into()),
            lines: vec![Line {
                speaker: Speaker::OpenCode,
                text: "Part".into(),
                partial: true,
            }],
            more: false,
            finished: None,
        }),
    );
    for (snapshot, known) in [(saved_before_ack, false), (app.snapshot(), true)] {
        let mut reopened = Workspace::default();
        reopened.remote.saved = snapshot.remote;
        reopened.select_remote(&id);
        let (req, _) = reopened.commands().pop().unwrap();
        finish(
            &mut reopened,
            req,
            Outcome::Prepared(machine("vm-a"), "feat/new".into()),
        );
        let mut work = reopened.commands();
        if known {
            let (req, Command::Remote(Operation::Follow(_, restored, cursor))) =
                work.pop().unwrap()
            else {
                panic!()
            };
            assert_eq!(restored, run);
            assert_eq!(cursor.as_deref(), Some("after"));
            finish(
                &mut reopened,
                req,
                Outcome::Page(Page {
                    cursor: Some("end".into()),
                    lines: vec![Line {
                        speaker: Speaker::OpenCode,
                        text: "Full reply".into(),
                        partial: false,
                    }],
                    more: false,
                    finished: Some("finished".into()),
                }),
            );
            assert_eq!(reopened.remote.saved.checkouts[&id].thread.lines.len(), 2);
            assert!(reopened.send_remote(&id, "Continue"));
            assert!(
                matches!(reopened.commands().as_slice(),[(_,Command::Remote(Operation::Prompt(_,Some(c),_)))] if c=="c")
            );
        } else {
            assert!(work.is_empty());
            assert!(!reopened.send_remote(&id, "Edit a file"));
        }
    }
}

#[test]
fn only_the_visible_monitor_polls_and_stale_samples_cannot_stop_a_new_attempt() {
    use std::time::{Duration, Instant};
    let mut app = Workspace::default();
    let (id, req, _) = created(&mut app);
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Machine(machine("vm-a")))),
    ));
    let (req, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Prepared(
            machine("vm-a"),
            "feat/new".into(),
        ))),
    ));
    let now = Instant::now();
    assert!(app.monitor_remote(false, now).is_none() && app.commands().is_empty());
    let sample = || {
        Outcome::Stats(Sample {
            machine: machine("vm-a"),
            vcpu: Some(2),
            memory_gb: Some(4),
            activity: Activity::Unavailable,
        })
    };
    app.monitor_remote(true, now);
    let (old, _) = app.commands().pop().unwrap();
    app.monitor_remote(false, now);
    app.monitor_remote(true, now);
    let (new, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(old, Ok(Reply::Remote(sample()))));
    assert!(app.remote.monitor.sample(&id).is_none());
    app.apply(Event::Completed(new, Ok(Reply::Remote(sample()))));
    let next = app.monitor_remote(true, now).unwrap();
    assert!(app.commands().is_empty());
    assert!(
        app.monitor_remote(false, next + Duration::from_secs(60))
            .is_none()
            && app.commands().is_empty()
    );
}
