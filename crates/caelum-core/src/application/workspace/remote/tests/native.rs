use super::*;
use crate::application::{Options, Provider, Scope, SessionKey, Summary, Task};
use crate::remote::native::{project_key, Binding};
use caelum_protocol::{ApprovalDecision, ApprovalPrompt, QuestionPrompt};

#[test]
fn colliding_machine_paths_and_session_ids_keep_independent_editors_brokers_and_commands() {
    let mut app = Workspace::default();
    let path = "/workspace/checkout";
    let scope = |project: &str| Scope {
        project: project.into(),
        provider: Provider::Codex,
    };
    app.projects.push(caelum_protocol::ProjectInfo {
        name: "Laptop".into(),
        path: path.into(),
    });
    let local = app.new_session(scope(path));
    app.sessions[local].remote_id = Some("ses_same".into());
    app.sessions[local].draft = "local draft".into();
    let mut bindings = vec![];
    for vm in ["vm-a", "vm-b"] {
        let (id, req, _) = created(&mut app);
        app.apply(Event::Completed(
            req,
            Ok(Reply::Remote(prepared_vm(machine(vm), "main"))),
        ));
        let c = app.remote.saved.checkouts.get_mut(&id).unwrap();
        c.source.project = path.into();
        let binding = Binding::new(&id, &c.native_target().unwrap()).unwrap();
        let i = app.open_vm_session(&id, SessionIntent::New).unwrap();
        app.sessions[i].draft = vm.into();
        app.discover_sessions(scope(&binding.project));
        let req = app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c,Command::List(s) if s.project==binding.project))
            .unwrap()
            .0;
        let event = Event::Completed(
            req,
            Ok(Reply::Sessions(vec![Summary {
                updated_at: None,
                key: SessionKey {
                    scope: scope(path),
                    id: "ses_same".into(),
                },
                title: vm.into(),
                working: false,
                options: Options::default(),
            }])),
        );
        app.apply(binding.incoming(event).unwrap());
        bindings.push((i, binding));
    }
    assert_eq!(app.sidebar_sessions(Some(path), "", false).len(), 5);
    assert!(app.commands().iter().all(|(_, c)| !matches!(
        c,
        Command::VerifyOpenCodeMove { .. } | Command::Start(..) | Command::Send(..)
    )));
    let approval = |p: &str| ApprovalPrompt {
        project_path: p.into(),
        provider: "codex".into(),
        session_id: Some("ses_same".into()),
        request_id: "per_same".into(),
        summary: "test".into(),
        description: "".into(),
    };
    app.apply(Event::Approvals(vec![approval(path)]));
    for (_, binding) in &bindings {
        let mut event = binding
            .incoming(Event::Approvals(vec![approval(path)]))
            .unwrap();
        app.merge_broker_event(Some(&binding.project), &mut event);
        app.apply(event);
        let question = QuestionPrompt {
            provider: "codex".into(),
            project_path: path.into(),
            session_id: "ses_same".into(),
            request_id: "que_same".into(),
            title: "Destination?".into(),
            fields: vec![],
        };
        let mut event = binding.incoming(Event::Questions(vec![question])).unwrap();
        app.merge_broker_event(Some(&binding.project), &mut event);
        app.apply(event);
    }
    let mut clear = Event::Approvals(vec![]);
    app.merge_broker_event(None, &mut clear);
    app.apply(clear);
    assert_eq!(app.approvals.len(), 2);
    assert_eq!(app.questions.len(), 2);
    for (i, binding) in &bindings {
        app.select(*i);
        assert_eq!(
            app.approvals
                .iter()
                .filter(|p| p.project_path == binding.project)
                .count(),
            1
        );
        let file = || Task::File {
            project: binding.project.clone(),
            path: "readme".into(),
        };
        assert!(
            matches!(binding.outgoing(Command::Task(file())).unwrap(),Command::Task(Task::File{project,..}) if project==path)
        );
        assert!(binding.outgoing(Command::Open(path.into())).is_err());
        let question = app
            .questions
            .iter()
            .find(|q| q.project_path == binding.project)
            .unwrap()
            .clone();
        let approval = app
            .approvals
            .iter()
            .find(|p| p.project_path == binding.project)
            .unwrap()
            .clone();
        app.approve(approval, ApprovalDecision::Deny);
        app.answer(question, None).unwrap();
        let replies: Vec<_> = app
            .commands()
            .into_iter()
            .filter_map(|(_, c)| match c {
                Command::Answer(..) | Command::Approve(..) => Some(binding.outgoing(c).unwrap()),
                _ => None,
            })
            .collect();
        assert!(
            matches!(&replies[..], [Command::Approve(a,ApprovalDecision::Deny), Command::Answer(q,None)]
            if a.project_path == path && q.project_path == path && q.session_id=="ses_same")
        );
    }
    assert_eq!(app.sessions[local].draft, "local draft");
    assert_eq!(app.sessions[bindings[0].0].draft, "vm-a");
    assert_eq!(app.sessions[bindings[1].0].draft, "vm-b");
}

#[test]
fn disconnect_while_saving_does_not_release_an_old_turn_on_the_next_attachment() {
    let mut app = Workspace::default();
    let (id, create, _) = created(&mut app);
    app.apply(Event::Completed(
        create,
        Ok(Reply::Remote(prepared_vm(machine("vm"), "main"))),
    ));
    let i = app.open_vm_session(&id, SessionIntent::New).unwrap();
    let project = app.sessions[i].scope.project.clone();
    app.sessions[i].remote_id = Some("thread-vm".into());
    app.enable_store("workspace.json".into());
    let load = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(load, Ok(Reply::Snapshot(None))));
    assert!(app.send(i, "One explicit turn".into()));
    let commands = app.commands();
    let [(save, Command::Save(_, snapshot))] = commands.as_slice() else {
        panic!("{commands:?}")
    };
    assert!(snapshot.sessions[i].pending_submission.is_some());
    for event in app.native_connection_lost(&project) {
        app.apply(event);
    }
    app.sessions[i].draft = "Keep editing offline".into();
    app.native_connected(&project);
    app.apply(Event::Completed(*save, Ok(Reply::Done)));
    // Drain the subsequent persistence barrier too.
    for _ in 0..2 {
        for (req, cmd) in app.commands() {
            match cmd {
                Command::Save(..) => app.apply(Event::Completed(req, Ok(Reply::Done))),
                Command::Send(..) | Command::Start(..) | Command::Enqueue(..) => {
                    panic!("replayed {cmd:?}")
                }
                _ => {}
            }
        }
    }
    assert_eq!(
        app.sessions[i].failed_prompt.as_deref(),
        Some("One explicit turn")
    );
    assert_eq!(app.sessions[i].draft, "Keep editing offline");
    assert!(app.sessions[i].queue_paused);
}

#[test]
fn confirmed_deletion_retires_an_inflight_native_start_and_late_discovery() {
    let mut app = Workspace::default();
    let (id, create, _) = created(&mut app);
    let vm = machine("vm");
    app.apply(Event::Completed(
        create,
        Ok(Reply::Remote(prepared_vm(vm.clone(), "main"))),
    ));
    let i = app.open_vm_session(&id, SessionIntent::New).unwrap();
    let scope = app.sessions[i].scope.clone();
    assert!(app.send(i, "work".into()));
    app.discover_sessions(scope.clone());
    let commands = app.commands();
    let start = commands
        .iter()
        .find(|(_, c)| matches!(c, Command::Start(..)))
        .unwrap()
        .0;
    let list = commands
        .iter()
        .find(|(_, c)| matches!(c, Command::List(_)))
        .unwrap()
        .0;
    assert!(app.remove_remote(&id, Some(&vm.key)));
    let delete = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(delete, Err("Boat refused".into())));
    assert!(!app.sessions[i].forgotten() && app.sessions[i].working());
    assert!(app.remove_remote(&id, Some(&vm.key)));
    let delete = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(
        delete,
        Ok(Reply::Remote(Outcome::Deleted(vm.key))),
    ));
    let summary = Summary {
        key: SessionKey {
            scope,
            id: "retired".into(),
        },
        title: "late".into(),
        working: false,
        updated_at: None,
        options: Options::default(),
    };
    app.apply(Event::Completed(
        list,
        Ok(Reply::Sessions(vec![summary.clone()])),
    ));
    app.apply(Event::Completed(start, Ok(Reply::Session(summary))));
    assert!(app.snapshot().sessions.is_empty());
    assert!(app.commands().is_empty());
}

#[test]
fn former_vm_profile_is_imported_once_without_replacing_the_laptop_selection() {
    use crate::application::Store;
    let tmp = tempfile::tempdir().unwrap();
    let main = Store(tmp.path().join("state.json"));
    let mut app = Workspace::default();
    let (id, req, _) = created(&mut app);
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(prepared_vm(machine("vm-a"), "main"))),
    ));
    let target = app.remote.saved.checkouts[&id].native_target().unwrap();
    let local = app.new_session(Scope {
        project: "/local".into(),
        provider: Provider::Codex,
    });
    app.sessions[local].draft = "on laptop".into();
    main.save(&app.snapshot()).unwrap();
    let old = Store(target.store_path(&main.0));
    let mut legacy = Workspace::default();
    let i = legacy.new_session(Scope {
        project: "/previous-checkout-path".into(),
        provider: Provider::OpenCode,
    });
    legacy.sessions[i].remote_id = Some("ses_prior".into());
    legacy.sessions[i].draft = "in VM editor".into();
    legacy.sessions[i].options.model = Some("custom/model".into());
    let unrelated = legacy.new_session(Scope {
        project: "/unrelated-project".into(),
        provider: Provider::Codex,
    });
    legacy.sessions[unrelated].draft = "leave this session out".into();
    legacy.preferences.default_provider = Provider::OpenCode;
    legacy
        .preferences
        .providers
        .insert("opencode".into(), legacy.sessions[i].options.clone());
    legacy
        .preferences
        .favorite_models
        .insert("opencode".into(), vec!["vm-only".into()]);
    old.save(&legacy.snapshot()).unwrap();
    for _ in 0..2 {
        let state = main.load_workspace().unwrap().unwrap();
        assert_eq!(state.sessions.len(), 2);
        assert_eq!(state.selected, Some(local));
        let defaults = state.remote.checkouts[&id].defaults.as_ref().unwrap();
        assert_eq!(defaults.options["opencode"], legacy.sessions[i].options);
        assert_eq!(defaults.favorites["opencode"], ["vm-only"]);
        assert_eq!(defaults.provider, Provider::OpenCode);
        assert_eq!(state.preferences.default_provider, Provider::Codex);
        assert!(
            state.preferences.providers.is_empty() && state.preferences.favorite_models.is_empty()
        );
        assert_eq!(state.sessions[1].scope.project, project_key(&id));
        assert_eq!(state.sessions[1].scope.provider, Provider::OpenCode);
        assert_eq!(state.sessions[1].remote_id.as_deref(), Some("ses_prior"));
        assert_eq!(state.sessions[1].draft, "in VM editor");
        assert_eq!(
            state.sessions[1].options.model.as_deref(),
            Some("custom/model")
        );
    }
}

#[test]
fn saved_opencode_handoff_keeps_its_owner_settings_and_draft_during_codex_discovery() {
    let mut app = Workspace::default();
    let (id, req, _) = created(&mut app);
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(prepared_vm(machine("vm"), "main"))),
    ));
    let checkout = app.remote.saved.checkouts.get_mut(&id).unwrap();
    checkout.defaults = Some(ProviderDefaults {
        provider: Provider::OpenCode,
        ..Default::default()
    });
    checkout.thread = Thread {
        awaiting_native: true,
        provider: Provider::OpenCode,
        model: Some("earlier/model".into()),
        draft: "unsent".into(),
        continued: Some(Continuation {
            id: "handoff-before-upgrade".into(),
            title: "Original handoff".into(),
            chat: Chat {
                messages: vec![Message::Assistant("Earlier answer".into())],
                clipped: false,
            },
            base: None,
        }),
        ..Default::default()
    };
    let slot = app.open_vm_session(&id, SessionIntent::Reopen).unwrap();
    let scope = app.sessions[slot].scope.clone();
    assert_eq!(scope.provider, Provider::OpenCode);
    assert_eq!(
        app.sessions[slot].conversation.entries[0].text,
        "Earlier answer"
    );
    assert_eq!(
        app.sessions[slot].options.model.as_deref(),
        Some("earlier/model")
    );
    app.sessions[slot].remote_id = Some("ses_prior".into());
    let original = app.snapshot().sessions[slot].clone();
    assert!(!app.send(slot, "unsent".into()));
    app.native_connected(&scope.project);
    app.refresh_session(slot);
    app.refresh_choices_for(scope.clone());
    app.discover_sessions(scope.clone());
    app.refresh_inbox(slot);
    let cmds = app.commands();
    assert!(cmds.iter().any(|(_, c)| matches!(c, Command::List(s) if s.provider == Provider::Codex && s.project == scope.project)));
    assert!(
        cmds.iter().all(|(_, c)| match c {
            Command::List(s) | Command::ListArchived(s) | Command::Choices(s) =>
                s.provider == Provider::Codex,
            Command::Task(Task::Git { project } | Task::Directory { project, .. }) =>
                project == &scope.project,
            _ => false,
        }),
        "{cmds:?}"
    );
    assert_eq!(app.open_vm_session(&id, SessionIntent::Reopen), Some(slot));
    let fresh = app.open_vm_session(&id, SessionIntent::New).unwrap();
    assert_ne!(fresh, slot);
    assert_eq!(app.sessions[fresh].scope.provider, Provider::Codex);
    assert!(app.sessions[fresh].remote_id.is_none() && app.sessions[fresh].draft.is_empty());
    assert_eq!(
        app.default_provider_for(Some(&scope.project)),
        Provider::Codex
    );
    assert_eq!(app.snapshot().sessions[slot], original);
    assert!(app.commands().is_empty());
}

#[test]
fn vm_routing_refuses_opencode_operations_even_with_a_codex_session_key() {
    let mut app = Workspace::default();
    let (id, req, _) = created(&mut app);
    app.apply(Event::Completed(
        req,
        Ok(Reply::Remote(prepared_vm(machine("vm"), "main"))),
    ));
    let binding = Binding::new(
        &id,
        &app.remote.saved.checkouts[&id].native_target().unwrap(),
    )
    .unwrap();
    let project = binding.project.clone();
    let opencode = Scope {
        project: project.clone(),
        provider: Provider::OpenCode,
    };
    let key = SessionKey {
        scope: Scope {
            provider: Provider::Codex,
            ..opencode.clone()
        },
        id: "thread-existing".into(),
    };
    for mut cmd in [
        Command::Choices(opencode.clone()),
        Command::Start(opencode.clone(), Options::default()),
        Command::Send(
            SessionKey {
                scope: opencode,
                id: "ses_existing".into(),
            },
            "draft".into(),
            Options::default(),
        ),
        Command::Commands(key.scope.clone()),
        Command::Shells(key.clone()),
        Command::Enqueue(key.clone(), "queued".into(), "text".into()),
        Command::Generate(key, "side question".into()),
        Command::Task(Task::Usage {
            project: project.clone(),
            provider: Provider::OpenCode,
        }),
        Command::Task(Task::Image {
            project: project.clone(),
            source: "opencode-media:old".into(),
        }),
        Command::Approve(
            ApprovalPrompt {
                provider: "opencode".into(),
                project_path: project.clone(),
                session_id: None,
                request_id: "same".into(),
                summary: "saved".into(),
                description: String::new(),
            },
            ApprovalDecision::Deny,
        ),
    ] {
        assert_eq!(Binding::owner(&mut cmd).as_deref(), Some(&project));
        assert!(binding.outgoing(cmd).is_err());
    }
}
