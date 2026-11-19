use super::*;
use crate::application::{Attachment, HandoffPhase, HandoffSource, Provider, Scope, VmDestination};
use caelum_protocol::ProjectInfo;

fn local(app: &mut Workspace) -> usize {
    let project = source().project;
    app.projects.push(ProjectInfo {
        path: project.clone(),
        name: "Project".into(),
    });
    app.remote.inventory = Some(Inventory {
        account: "owner".into(),
        machines: vec![machine("vm")],
    });
    let i = app.new_session(Scope {
        project,
        provider: Provider::OpenCode,
    });
    let s = &mut app.sessions[i];
    s.remote_id = Some("local-session".into());
    s.draft = "Next step".into();
    s.attachments.push(Attachment::new(
        "local.rs".into(),
        "local attachment".into(),
        None,
        false,
    ));
    i
}

fn reading(app: &mut Workspace, i: usize) -> u64 {
    assert!(app.begin_vm_handoff(HandoffSource::Composer(i)));
    app.commands()
        .into_iter()
        .find_map(|(req, c)| {
            matches!(c, Command::HandoffChat(key) if key.id == "local-session").then_some(req)
        })
        .unwrap()
}

fn ready(app: &mut Workspace, req: u64) {
    app.apply(Event::Completed(
        req,
        Ok(Reply::HandoffChat(Chat {
            messages: vec![
                Message::User("Earlier question".into()),
                Message::Assistant("Earlier answer".into()),
            ],
            clipped: false,
        })),
    ));
    app.remote.picker = Some(Picker::Ready(source()));
}

fn new_vm() -> VmDestination {
    VmDestination::Create {
        base: "main".into(),
        branch: "feat/next".into(),
        size: Size::Small,
        minutes: 30,
    }
}

#[test]
fn late_history_and_source_changes_cannot_provision_or_replace_a_newer_attempt() {
    let mut app = Workspace::default();
    let i = local(&mut app);
    let old = reading(&mut app, i);
    app.dismiss_vm_handoff();
    let current = reading(&mut app, i);
    ready(&mut app, old);
    assert!(matches!(app.vm_handoff().unwrap().phase, HandoffPhase::Reading(r) if r == current));
    assert!(app.finish_vm_handoff(new_vm()).is_none());
    app.leave_conversation();
    ready(&mut app, current);
    assert!(app.finish_vm_handoff(new_vm()).is_none());
    app.select(i);
    let req = reading(&mut app, i);
    app.apply(Event::Completed(req, Err("unavailable".into())));
    assert!(matches!(
        app.vm_handoff().unwrap().phase,
        HandoffPhase::Failed(_)
    ));
    assert!(app.finish_vm_handoff(new_vm()).is_none());
    let req = reading(&mut app, i);
    ready(&mut app, req);
    app.remote.picker = Some(Picker::Ready(Source {
        project: "/different".into(),
        ..source()
    }));
    assert!(app.finish_vm_handoff(new_vm()).is_none());
    app.sessions[i].conversation.working = true;
    assert!(!app.begin_vm_handoff(HandoffSource::Composer(i)));
    assert!(app.remote.saved.checkouts.is_empty() && app.commands().is_empty());
    assert_eq!(app.sessions[i].draft, "Next step");
}

#[test]
fn handoff_first_send_survives_rejection_and_restore_without_replaying_chat_after_ack() {
    let mut app = Workspace::default();
    let i = local(&mut app);
    let req = reading(&mut app, i);
    ready(&mut app, req);
    app.sessions[i].draft = "New laptop draft".into();
    let id = app.finish_vm_handoff(new_vm()).unwrap();
    assert_eq!(app.sessions[i].draft, "New laptop draft");
    assert_eq!(app.sessions[i].attachments.len(), 1);
    let creates = app.commands();
    let [(req, Command::Remote(Operation::Create(_, _)))] = creates.as_slice() else {
        panic!()
    };
    app.apply(Event::Completed(*req, Err("network offline".into())));
    assert!(app.commands().is_empty());
    let mut reopened = Workspace::default();
    reopened.remote.saved =
        serde_json::from_value(serde_json::to_value(app.snapshot().remote).unwrap()).unwrap();
    reopened.select_remote(&id);
    let (retry, Command::Remote(Operation::Create(_, plan))) = reopened.commands().pop().unwrap()
    else {
        panic!()
    };
    assert_eq!(plan.token, id);
    let complete = |app: &mut Workspace, req, result| {
        app.apply(Event::Completed(req, Ok(Reply::Remote(result))))
    };
    complete(&mut reopened, retry, Outcome::Machine(machine("vm")));
    let (prepare, _) = reopened.commands().pop().unwrap();
    complete(
        &mut reopened,
        prepare,
        Outcome::Prepared(machine("vm"), "feat/next".into()),
    );
    assert!(reopened.commands().is_empty());
    assert_eq!(
        reopened.remote.saved.checkouts[&id].thread.draft,
        "Next step"
    );
    for reject in [true, false] {
        assert!(reopened.send_remote(&id, "Edited next step"));
        let (req, Command::Remote(Operation::Prompt(target, None, prompt))) =
            reopened.commands().pop().unwrap()
        else {
            panic!()
        };
        assert_eq!(target.machine.id, "vm");
        assert!(
            prompt.contains("Earlier question")
                && prompt.contains("Earlier answer")
                && prompt.ends_with("Edited next step")
        );
        assert!(!prompt.contains("local attachment") && !prompt.contains("local-session"));
        assert_eq!(
            reopened.remote.saved.checkouts[&id]
                .thread
                .lines
                .last()
                .unwrap()
                .text,
            "Edited next step"
        );
        if reject {
            complete(&mut reopened, req, Outcome::Rejected("try again".into()));
            assert_eq!(
                reopened.remote.saved.checkouts[&id].thread.draft,
                "Edited next step"
            );
        } else {
            let saved = reopened.snapshot().remote;
            reopened = Workspace::default();
            reopened.remote.saved = saved;
            assert!(!reopened.send_remote(&id, "duplicate"));
            reopened.recover_remote(&id, "p");
            let (req, Command::Remote(Operation::Recover(_, None, _))) =
                reopened.commands().pop().unwrap()
            else {
                panic!()
            };
            complete(
                &mut reopened,
                req,
                Outcome::Accepted(Run {
                    conversation: "boat-thread".into(),
                    id: "r".into(),
                    prompt: "p".into(),
                }),
            );
            let (req, _) = reopened.commands().pop().unwrap();
            complete(
                &mut reopened,
                req,
                Outcome::Page(Page {
                    lines: vec![],
                    cursor: None,
                    more: false,
                    finished: Some("finished".into()),
                }),
            );
            assert!(reopened.send_remote(&id, "Again"));
            assert!(
                matches!(reopened.commands().as_slice(), [(_, Command::Remote(Operation::Prompt(_, Some(c), p)))] if c == "boat-thread" && p == "Again")
            );
        }
    }
}

#[test]
fn home_can_continue_on_an_existing_idle_machine_without_overwriting_its_thread() {
    let mut app = Workspace::default();
    let i = local(&mut app);
    let m = machine("vm");
    app.remote.picker = Some(Picker::Ready(source()));
    let id = app.attach_remote(m.clone()).unwrap();
    let (req, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Prepared(
            m.clone(),
            "existing".into(),
        ))),
    ));
    let original = Thread {
        conversation: Some("original".into()),
        draft: "VM draft".into(),
        ..Default::default()
    };
    app.remote.saved.checkouts.get_mut(&id).unwrap().thread = original.clone();
    app.home_select_project(source().project);
    app.home.draft.text = "From Home".into();
    assert!(app.begin_vm_handoff(HandoffSource::HomeDraft));
    app.home_select_project(source().project);
    app.remote.picker = Some(Picker::Ready(source()));
    assert!(app
        .finish_vm_handoff(VmDestination::Existing(m.clone()))
        .is_none());
    assert!(app.begin_vm_handoff(HandoffSource::HomeDraft));
    app.remote.picker = Some(Picker::Ready(source()));
    let destination = || VmDestination::Existing(m.clone());
    app.remote.saved.checkouts.get_mut(&id).unwrap().thread.turn = Turn::Submitting;
    assert!(app.finish_vm_handoff(destination()).is_none());
    app.remote.saved.checkouts.get_mut(&id).unwrap().thread.turn = Turn::Idle;
    assert_eq!(app.finish_vm_handoff(destination()), Some(id.clone()));
    assert_eq!(app.home.draft.text, "From Home");
    assert_eq!(app.sessions[i].draft, "Next step");
    let checkout = &app.remote.saved.checkouts[&id];
    assert_eq!(checkout.other_threads, std::slice::from_ref(&original));
    assert_eq!(checkout.thread.draft, "From Home");
    assert!(checkout.thread.conversation.is_none());
    let new_key = checkout.thread.editor_key(&id).to_owned();
    let (req, _) = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Remote(Operation::Prepare(..))))
        .unwrap();
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Prepared(m, "existing".into()))),
    ));
    assert!(app.switch_vm_thread(&id, &id));
    assert_eq!(app.remote.saved.checkouts[&id].thread, original);
    assert!(app.switch_vm_thread(&id, &new_key));
    assert!(app.send_remote(&id, "From Home"));
    let commands = app.commands();
    let [(req, Command::Remote(Operation::Prompt(_, None, prompt)))] = commands.as_slice() else {
        panic!()
    };
    assert_eq!(prompt, "From Home");
    assert!(!app.switch_vm_thread(&id, &id));
    app.apply(Event::Completed(*req, Err("lost acknowledgement".into())));
    app.recover_remote(&id, "old-prompt");
    let (req, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(Outcome::Accepted(Run {
            conversation: "original".into(),
            prompt: "old-prompt".into(),
            id: "old-run".into(),
        }))),
    ));
    assert!(app.commands().is_empty());
    assert!(app.remote.saved.checkouts[&id]
        .thread
        .conversation
        .is_none());
    assert_eq!(
        app.remote.saved.checkouts[&id].thread.turn,
        Turn::Submitting
    );
}
