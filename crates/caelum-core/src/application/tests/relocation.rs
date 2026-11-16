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
        Ok(Reply::SessionMoved {
            from,
            project: ProjectInfo {
                path: path.into(),
                name: "New checkout".into(),
            },
        }),
    ));
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
    assert!(session.loading);
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
    // A duplicate completion cannot rebind the session a second time.
    moved(&mut app, request, key, "/stale");
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.sessions[index].scope.project, "/new");
    assert!(app.sessions[index].loading);
    assert!(app.sessions[index].error.is_none());
    assert!(app.sessions[index].inbox.items.is_empty());
}

#[test]
fn destination_discovery_reconnects_the_saved_session_instead_of_creating_another() {
    let (mut app, index) = fixture();
    let new_scope = scope(Provider::OpenCode, "/new");
    let request = discover_session_list(&mut app, &new_scope, false);
    app.apply(Event::Completed(
        request,
        Ok(Reply::Sessions(vec![summary(new_scope, "ses_moved")])),
    ));
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(
        app.sessions[index].scope.project, "/old",
        "discovery alone does not authorize a move"
    );
    assert!(app.commands().iter().any(
        |(_, command)| matches!(command, Command::Resume(key) if key.scope.project == "/old")
    ));
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
    assert!(!app.sessions[index].forgotten);
    assert!(!app.sessions[index].archived);
    assert!(!app.sessions[index].archiving);
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
        assert!(!app.sessions[index].loading);
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
    app.sessions[index].remote_id = Some(id);
    app.sessions[index].loaded = false;
    app.refresh(index);
    let (wake, wakes) = mpsc::channel();
    let mut runtime = Runtime::new(move || {
        let _ = wake.send(());
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        for event in runtime.drain() {
            app.apply(event);
        }
        // No persisted workspace, prompts, archive, title, or control commands.
        for (request, command) in app.commands() {
            if matches!(command, Command::Resume(_)) {
                runtime.submit(request, command).unwrap();
            }
        }
        let session = &app.sessions[index];
        assert!(
            session.error.is_none(),
            "reconnection failed: {:?}",
            session.error
        );
        if session.loaded && !session.loading {
            assert_eq!(session.scope.project, destination);
            assert_eq!(app.selected, Some(index));
            assert_eq!(app.sessions.len(), 1);
            assert!(!session.conversation.entries.is_empty());
            break;
        }
        wakes
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap();
    }
}
