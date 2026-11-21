use super::*;

fn fixture() -> (Workspace, usize) {
    let mut app = Workspace::default();
    app.projects.push(ProjectInfo {
        path: "/project".into(),
        name: "Project".into(),
    });
    app.project_roots.insert("/old".into(), "/project".into());
    let index = app.new_session(scope(Provider::OpenCode, "/old"));
    let session = &mut app.sessions[index];
    session.remote_id = Some("ses_moved".into());
    session.draft = "Keep the draft".into();
    session.reading = 120.0;
    session.options.model = Some("local/chosen".into());
    session.attachments.push(Attachment::new(
        "example.rs".into(),
        "snapshot".into(),
        None,
        false,
    ));
    session.conversation.history(
        Provider::OpenCode,
        &[json!({"id":"msg_old", "type":"user", "text":"History"})],
        false,
    );
    (app, index)
}

fn refresh(app: &mut Workspace, index: usize) -> (u64, SessionKey) {
    app.refresh(index);
    app.commands()
        .into_iter()
        .find_map(|(id, command)| match command {
            Command::Resume(key) => Some((id, key)),
            _ => None,
        })
        .unwrap()
}

fn moved(app: &mut Workspace, request: u64, from: SessionKey, path: &str) {
    app.apply(Event::Completed(
        request,
        Ok(Reply::SessionMoved(OpenCodeMove {
            from,
            linked_worktree: true,
            archived: false,
            project: ProjectInfo {
                path: path.into(),
                name: "New checkout".into(),
            },
        })),
    ));
}

fn move_event(app: &mut Workspace, listener: &str, destination: &str) {
    app.apply(Event::OpenCode(OpenCodeEvent {
        project_path: listener.into(),
        event_type: "session.moved".into(),
        data: json!({"sessionID":"ses_moved", "location":{"directory":destination}}),
        id: None,
        created: None,
        location: None,
    }));
}

fn move_check(app: &mut Workspace, path: &str) -> (u64, OpenCodeMove) {
    let mut checks = app
        .commands()
        .into_iter()
        .filter_map(|(id, command)| match command {
            Command::VerifyOpenCodeMove { from, destination } => {
                assert_eq!(destination, path);
                assert_eq!(from.scope.project, "/old");
                Some((
                    id,
                    OpenCodeMove {
                        from,
                        project: ProjectInfo {
                            path: destination,
                            name: "Destination".into(),
                        },
                        linked_worktree: false,
                        archived: false,
                    },
                ))
            }
            _ => None,
        });
    let check = checks.next().unwrap();
    assert!(checks.next().is_none(), "duplicate verification");
    check
}

#[test]
fn moved_session_preserves_its_local_slot_and_reconnects_in_the_verified_checkout() {
    let (mut app, index) = fixture();
    app.sessions[index].queue.push(QueuedPrompt {
        text: "Queued for old checkout".into(),
        options: Options::default(),
    });
    let (request, key) = refresh(&mut app, index);
    moved(&mut app, request, key, "/new");

    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.selected, Some(index));
    assert_eq!(app.active_project.as_deref(), Some("/new"));
    assert_eq!(app.group_path("/new"), "/project");
    let session = &app.sessions[index];
    assert_eq!(session.scope.project, "/new");
    assert_eq!(session.remote_id.as_deref(), Some("ses_moved"));
    assert_eq!(session.draft, "Keep the draft");
    assert_eq!(session.reading, 120.0);
    assert_eq!(session.attachments[0].content, "snapshot");
    assert_eq!(session.options.model.as_deref(), Some("local/chosen"));
    assert_eq!(session.conversation.entries[0].text, "History");
    assert_eq!(session.queue.len(), 1);
    assert!(session.queue_paused);
    assert!(session.loading());
    let commands = app.commands();
    assert!(commands.iter().any(|(_, command)| matches!(command, Command::Resume(key) if key.scope.project == "/new" && key.id == "ses_moved")));
    assert!(!commands.iter().any(|(_, command)| matches!(
        command,
        Command::Send(..) | Command::Archive(..) | Command::DeleteSession(..)
    )));
    let saved = app.snapshot();
    assert_eq!(saved.sessions[0].scope.project, "/new");
    assert_eq!(saved.remembered_checkouts.get("/new"), Some(&index));
    assert!(!saved.remembered_checkouts.contains_key("/old"));
    assert!(
        saved.managed_worktrees.is_empty(),
        "external worktrees remain externally owned"
    );
}

#[test]
fn reconnect_discards_old_checkout_reads_and_late_lists_cannot_duplicate_the_session() {
    let (mut app, index) = fixture();
    let stale_list = discover_session_list(&mut app, &scope(Provider::OpenCode, "/old"), false);
    app.refresh_inbox(index);
    let inbox = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Inbox(_)))
        .unwrap()
        .0;
    let (request, key) = refresh(&mut app, index);
    moved(&mut app, request, key.clone(), "/new");
    app.apply(Event::Completed(inbox, Err("old ownership failure".into())));
    app.apply(Event::Completed(
        stale_list,
        Ok(Reply::Sessions(vec![summary(key.scope.clone(), &key.id)])),
    ));

    moved(&mut app, request, key, "/stale");
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.sessions[index].scope.project, "/new");
    assert!(app.sessions[index].loading());
    assert!(app.sessions[index].error.is_none());
    assert!(app.sessions[index].inbox.items.is_empty());
}

#[test]
fn destination_discovery_reconnects_the_saved_session_instead_of_creating_another() {
    let (mut app, index) = fixture();
    let new_scope = scope(Provider::OpenCode, "/new");
    app.projects.push(ProjectInfo {
        path: "/new".into(),
        name: "Destination".into(),
    });
    app.close_project("/project");
    let selected = app.selected;
    let count = app.sessions.len();
    let request = discover_session_list(&mut app, &new_scope, false);
    app.apply(Event::Completed(
        request,
        Ok(Reply::Sessions(vec![summary(new_scope, "ses_moved")])),
    ));
    assert_eq!(app.sessions.len(), count);
    assert_eq!(
        app.sessions[index].scope.project, "/old",
        "discovery alone does not authorize a move"
    );
    let (check, change) = move_check(&mut app, "/new");
    app.apply(Event::Completed(check, Ok(Reply::SessionMoved(change))));
    assert_eq!(app.sessions.len(), count);
    assert_eq!(app.sessions[index].scope.project, "/new");
    assert_eq!(app.group_path("/new"), "/new");
    assert_eq!(app.selected, selected);
    assert_eq!(app.sessions[index].draft, "Keep the draft");
    assert_eq!(app.sessions[index].reading, 120.0);
    assert!(app.session_feed(Some("/new"), "", false).contains(&index));
    assert!(app
        .commands()
        .iter()
        .any(|(_, cmd)| matches!(cmd, Command::Resume(key) if key.scope.project == "/new")));
}

#[test]
fn an_unopened_destination_is_named_but_not_added_or_resumed() {
    let (mut app, index) = fixture();
    let (request, from) = refresh(&mut app, index);
    app.apply(Event::Completed(
        request,
        Ok(Reply::SessionMoved(OpenCodeMove {
            from,
            linked_worktree: false,
            project: ProjectInfo {
                path: "/elsewhere".into(),
                name: "Destination".into(),
            },
            archived: false,
        })),
    ));
    assert_eq!(app.sessions[index].scope.project, "/old");
    assert!(app.sessions[index]
        .error
        .as_deref()
        .unwrap()
        .contains("/elsewhere"));
    assert!(!app.project_is_open("/elsewhere"));
    assert!(!app
        .commands()
        .iter()
        .any(|(_, cmd)| matches!(cmd, Command::Resume(_))));
}

#[test]
fn a_closed_destination_or_superseded_move_cannot_rebind_the_session() {
    let (mut app, index) = fixture();
    let mut checks = vec![];
    for path in ["/first", "/second"] {
        app.projects.push(ProjectInfo {
            path: path.into(),
            name: "Project".into(),
        });
        for _ in 0..2 {
            move_event(&mut app, path, path);
        }
        checks.push(move_check(&mut app, path));
    }
    app.close_project("/second");
    for (id, change) in checks.into_iter().rev() {
        app.apply(Event::Completed(id, Ok(Reply::SessionMoved(change))));
    }
    assert_eq!(app.sessions[index].scope.project, "/old");
    assert!(!app.project_is_open("/second"));
    assert!(app.commands().is_empty());
}

#[test]
fn the_latest_move_supersedes_a_verified_destination_waiting_for_a_mutation() {
    for last in ["/second", "/old"] {
        let (mut app, index) = fixture();
        for path in ["/first", last] {
            app.projects.push(ProjectInfo {
                path: path.into(),
                name: "Checkout".into(),
            });
        }
        app.archive(index, true);
        let archive = app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Archive(..)))
            .unwrap()
            .0;
        move_event(&mut app, "/old", "/first");
        let (first, change) = move_check(&mut app, "/first");
        app.apply(Event::Completed(first, Ok(Reply::SessionMoved(change))));
        assert_eq!(app.sessions[index].scope.project, "/old");
        move_event(&mut app, "/first", last);
        let (second, change) = move_check(&mut app, last);
        app.apply(Event::Completed(archive, Err("Stale checkout".into())));
        assert_eq!(app.sessions[index].scope.project, "/old");
        assert!(!app.sessions[index].can_submit());
        assert!(app.commands().is_empty());
        app.apply(Event::Completed(second, Ok(Reply::SessionMoved(change))));
        assert_eq!(app.sessions[index].scope.project, last);
        assert!(app.sessions[index].queue_paused);
        assert_eq!(app.sessions[index].draft, "Keep the draft");
        assert!(app
            .commands()
            .iter()
            .any(|(_, c)| matches!(c, Command::Resume(key) if key.scope.project == last)));
    }
}

#[test]
fn archive_state_comes_from_fresh_destination_metadata_even_if_an_older_list_arrives_late() {
    let (mut app, index) = fixture();
    app.projects.push(ProjectInfo {
        path: "/new".into(),
        name: "New".into(),
    });
    let scope = scope(Provider::OpenCode, "/new");
    let active = discover_session_list(&mut app, &scope, false);
    move_event(&mut app, "/old", "/new");
    let (check, mut change) = move_check(&mut app, "/new");
    change.archived = true;
    app.apply(Event::Completed(check, Ok(Reply::SessionMoved(change))));
    app.apply(Event::Completed(
        active,
        Ok(Reply::Sessions(vec![summary(scope, "ses_moved")])),
    ));
    assert_eq!(app.sessions[index].scope.project, "/new");
    assert!(app.sessions[index].archived());
    assert!(!app.session_feed(Some("/new"), "", false).contains(&index));
}

#[test]
fn reconnect_waits_for_local_mutations_and_retains_their_failure_before_retry() {
    let (mut app, index) = fixture();
    let (request, key) = refresh(&mut app, index);
    app.archive(index, true);
    let archive = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Archive(..)))
        .unwrap()
        .0;
    moved(&mut app, request, key, "/new");
    assert_eq!(app.sessions[index].scope.project, "/old");
    assert!(!app.sessions[index].can_submit());
    assert!(!app.sessions[index].can_send());
    app.apply(Event::Completed(
        archive,
        Err("OpenCode session does not belong to this project".into()),
    ));
    assert_eq!(app.sessions[index].scope.project, "/new");
    assert!(!app.sessions[index].forgotten());
    assert!(!app.sessions[index].archived());
    assert!(!app.sessions[index].archiving());
    assert_eq!(app.sessions[index].draft, "Keep the draft");
    assert!(app
        .commands()
        .iter()
        .any(|(_, c)| matches!(c, Command::Resume(_))));
}

#[test]
fn reconnect_preserves_newer_navigation_and_does_not_reopen_a_closed_project() {
    for close in [false, true] {
        let (mut app, index) = fixture();
        let (request, key) = refresh(&mut app, index);
        let other = app.new_session(scope(Provider::Codex, "/other"));
        if close {
            app.close_project("/project");
        }
        moved(&mut app, request, key, "/new");
        assert_eq!(app.selected, Some(other));
        assert_eq!(app.active_project.as_deref(), Some("/other"));
        assert_eq!(
            app.sessions[index].scope.project,
            if close { "/old" } else { "/new" }
        );
        assert_eq!(app.projects.iter().any(|p| p.path == "/new"), !close);
    }
}

#[test]
fn mismatched_relocation_reply_cannot_change_the_session_owner() {
    for from in [
        summary(scope(Provider::Codex, "/old"), "ses_moved").key,
        summary(scope(Provider::OpenCode, "/other"), "ses_moved").key,
        summary(scope(Provider::OpenCode, "/old"), "ses_other").key,
    ] {
        let (mut app, index) = fixture();
        let (request, _) = refresh(&mut app, index);
        moved(&mut app, request, from, "/new");
        assert_eq!(app.sessions[index].scope.project, "/old");
        assert!(app.sessions[index].error.is_some());
        assert!(!app.sessions[index].loading());
    }
}

#[test]
#[ignore = "read-only check against an explicitly selected, already-moved OpenCode session"]
fn real_moved_session_reconnection_smoke() {
    let source = std::env::var("CAELUM_SMOKE_SOURCE").expect("set the previous checkout");
    let destination = std::env::var("CAELUM_SMOKE_DESTINATION").expect("set the current checkout");
    let id = std::env::var("CAELUM_SMOKE_SESSION").expect("set the moved session ID");
    let mut app = Workspace::default();
    app.preferences.notifications = false;
    app.projects.push(ProjectInfo {
        path: source.clone(),
        name: "Smoke project".into(),
    });
    let index = app.new_session(scope(Provider::OpenCode, &source));
    app.sessions[index].remote_id = Some(id.clone());
    app.sessions[index].invalidate_history();
    let discover = std::env::var_os("CAELUM_SMOKE_DISCOVER_DESTINATION").is_some();
    if discover {
        app.projects.push(ProjectInfo {
            path: destination.clone(),
            name: "Destination".into(),
        });
        app.discover_sessions(scope(Provider::OpenCode, &destination));
    } else {
        app.refresh(index);
    }
    let (wake, wakes) = mpsc::channel();
    let mut runtime = Runtime::new(move || {
        let _ = wake.send(());
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        for event in runtime.drain() {
            app.apply(event);
        }

        for (request, command) in app.commands() {
            if matches!(&command, Command::Resume(key) if key.id == id)
                || (discover
                    && matches!(&command, Command::List(scope) if scope.project == destination))
                || (discover
                    && matches!(&command, Command::VerifyOpenCodeMove {from, destination: path} if from.id == id && *path == destination))
            {
                runtime.submit(request, command).unwrap();
            }
        }
        let session = &app.sessions[index];
        assert!(
            session.error.is_none(),
            "reconnection failed: {:?}",
            session.error
        );
        if session.loaded() && !session.loading() {
            assert_eq!(session.scope.project, destination);
            assert_eq!(app.selected, Some(index));
            assert_eq!(
                app.sessions
                    .iter()
                    .filter(|s| s.remote_id.as_ref() == Some(&id))
                    .count(),
                1
            );
            assert!(!session.conversation.entries.is_empty());
            break;
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap();
    }
}
