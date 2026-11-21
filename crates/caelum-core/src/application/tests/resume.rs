use super::*;

fn begin(workspace: &mut Workspace) -> (usize, u64, Summary) {
    let summary = summary(scope(Provider::OpenCode, "/a"), "ses_one");
    let index = workspace.new_session(summary.key.scope.clone());
    workspace.sessions[index].remote_id = Some(summary.key.id.clone());
    workspace.sessions[index].invalidate_history();
    workspace.refresh(index);
    let (request, command) = workspace.commands().pop().unwrap();
    assert!(matches!(command, Command::Resume(_)));
    (index, request, summary)
}

fn history(working: bool) -> SessionHistoryPage {
    SessionHistoryPage {
        data: vec![json!({"id":"msg_saved", "type":"user", "text":"Saved prompt"})],
        next_cursor: Some("older".into()),
        is_working: Some(working),
        model: Some("local/saved".into()),
        effort: Some("high".into()),
        agent: Some("build".into()),
    }
}

fn event(method: &str, mut data: serde_json::Value) -> Event {
    data["sessionID"] = "ses_one".into();
    Event::OpenCode(OpenCodeEvent {
        project_path: "/a".into(),
        location: None,
        event_type: method.into(),
        data,
        id: None,
        created: None,
    })
}

#[test]
fn bundled_resume_is_ready_without_a_history_command_and_recovers_inbox_before_queueing() {
    let mut workspace = Workspace::default();
    let (index, request, summary) = begin(&mut workspace);
    let delivered = json!({"id":"msg_saved", "type":"user", "payload":{"text":"Saved prompt"}});
    workspace.sessions[index]
        .inbox
        .snapshot(0, std::slice::from_ref(&delivered));
    workspace.sessions[index].queue.push(QueuedPrompt {
        text: "Next".into(),
        options: Options::default(),
    });
    workspace.apply(Event::Completed(
        request,
        Ok(Reply::Resumed(summary.clone(), history(false))),
    ));
    let session = &workspace.sessions[index];
    assert!(session.loaded());
    assert!(!session.loading());
    assert_eq!(session.conversation.entries[0].text, "Saved prompt");
    assert_eq!(session.next_cursor(), Some("older"));
    assert_eq!(session.options.model.as_deref(), Some("local/saved"));
    assert_eq!(session.options.effort.as_deref(), Some("high"));
    assert_eq!(session.options.agent.as_deref(), Some("build"));
    assert!(session.inbox.items.is_empty());
    assert_eq!(session.queue.len(), 1);
    let commands = workspace.commands();
    assert_eq!(commands.len(), 2);
    assert!(matches!(&commands[0].1, Command::Shells(key) if key == &summary.key));
    assert!(matches!(&commands[1].1, Command::Inbox(key) if key == &summary.key));
    workspace.apply(Event::Completed(
        commands[1].0,
        Ok(Reply::Inbox(vec![
            delivered,
            json!({"id":"msg_pending", "type":"user", "payload":{"text":"Still queued"}}),
        ])),
    ));
    assert_eq!(workspace.sessions[index].inbox.items.len(), 1);
    assert_eq!(workspace.sessions[index].inbox.items[0].id, "msg_pending");
    assert_eq!(workspace.sessions[index].queue.len(), 1);
    assert!(workspace.commands().is_empty());
}

#[test]
fn bundled_resume_preserves_navigation_live_text_activity_title_and_edited_options() {
    let mut workspace = Workspace::default();
    let (index, request, mut summary) = begin(&mut workspace);
    let other = workspace.new_session(scope(Provider::OpenCode, "/b"));
    workspace.sessions[other].remote_id = Some("ses_one".into());
    workspace.apply(event(
        "session.text.delta",
        json!({"assistantMessageID":"answer", "delta":"Live answer"}),
    ));
    workspace.apply(event("session.renamed", json!({"title":"Live title"})));
    workspace.sessions[index].options.model = Some("local/chosen".into());
    workspace.options_changed(index);
    summary.options.model = Some("local/stale".into());
    let mut page = history(false);
    page.data.push(
        json!({"id":"answer", "type":"assistant", "content":[{"type":"text", "text":"Live"}]}),
    );
    workspace.apply(Event::Completed(request, Ok(Reply::Resumed(summary, page))));
    let session = &workspace.sessions[index];
    assert!(session.working());
    assert!(session.loaded());
    assert_eq!(session.title, "Live title");
    assert_eq!(session.options.model.as_deref(), Some("local/chosen"));
    assert_eq!(session.conversation.entries.len(), 2);
    assert_eq!(session.conversation.entries[0].text, "Saved prompt");
    assert_eq!(session.conversation.entries[1].text, "Live answer");
    assert_eq!(workspace.selected, Some(other));
    assert!(workspace.sessions[other].conversation.entries.is_empty());
}

#[test]
fn bundled_resume_cannot_resurrect_cancelled_activity() {
    let mut workspace = Workspace::default();
    let (index, request, mut summary) = begin(&mut workspace);
    workspace.apply(event("session.execution.started", json!({})));
    workspace.cancel(index);
    let cancel = workspace
        .commands()
        .into_iter()
        .find(|(_, command)| matches!(command, Command::Cancel(_)))
        .unwrap()
        .0;
    workspace.apply(Event::Completed(cancel, Ok(Reply::Done)));
    summary.working = true;
    workspace.apply(Event::Completed(
        request,
        Ok(Reply::Resumed(summary, history(true))),
    ));
    assert!(!workspace.sessions[index].working());
    assert!(!workspace.sessions[index].stopping);
    assert!(workspace.sessions[index].queue_paused);
}

#[test]
fn bundled_resume_rejects_other_session_project_and_provider_before_applying_history() {
    for wrong in [
        summary(scope(Provider::OpenCode, "/a"), "ses_other"),
        summary(scope(Provider::OpenCode, "/b"), "ses_one"),
        summary(scope(Provider::Codex, "/a"), "ses_one"),
    ] {
        let mut workspace = Workspace::default();
        let (index, request, _) = begin(&mut workspace);
        workspace.apply(Event::Completed(
            request,
            Ok(Reply::Resumed(wrong, history(false))),
        ));
        let session = &workspace.sessions[index];
        assert!(!session.loaded());
        assert!(!session.loading());
        assert!(session
            .error
            .as_deref()
            .unwrap()
            .contains("different session"));
        assert!(session.conversation.entries.is_empty());
        assert!(workspace.commands().is_empty());
    }
}

#[test]
fn bundled_resume_failure_retries_and_ignores_duplicate_old_completions() {
    let mut workspace = Workspace::default();
    let (index, first, summary) = begin(&mut workspace);
    workspace.apply(Event::Completed(first, Err("history unavailable".into())));
    assert!(!workspace.sessions[index].loading());
    assert!(!workspace.sessions[index].loaded());
    assert_eq!(
        workspace.sessions[index].error.as_deref(),
        Some("history unavailable")
    );
    assert!(workspace.commands().is_empty());
    workspace.refresh(index);
    let retry = workspace.commands().pop().unwrap().0;
    workspace.apply(Event::Completed(
        first,
        Ok(Reply::Resumed(summary.clone(), history(false))),
    ));
    assert!(workspace.sessions[index].loading());
    assert!(!workspace.sessions[index].loaded());
    assert!(workspace.sessions[index].conversation.entries.is_empty());
    workspace.apply(Event::Completed(
        retry,
        Ok(Reply::Resumed(summary, history(false))),
    ));
    assert!(workspace.sessions[index].loaded());
    assert!(!workspace.sessions[index].loading());
    assert!(workspace.sessions[index].error.is_none());
}

#[test]
fn disconnect_retires_a_pending_history_reply_before_reconnect() {
    let mut workspace = Workspace::default();
    let (index, first, mut summary) = begin(&mut workspace);
    workspace.apply(event("server.disconnected", json!({})));
    let (reconnect, command) = workspace.commands().pop().unwrap();
    assert!(matches!(command, Command::Resume(_)));
    assert_ne!(first, reconnect);

    summary.working = true;
    workspace.apply(Event::Completed(
        first,
        Ok(Reply::Resumed(summary.clone(), history(true))),
    ));
    let session = &workspace.sessions[index];
    assert!(session.disconnected);
    assert!(session.loading());
    assert!(!session.working());
    assert!(session.conversation.entries.is_empty());

    summary.working = false;
    workspace.apply(Event::Completed(
        reconnect,
        Ok(Reply::Resumed(summary, history(false))),
    ));
    let session = &workspace.sessions[index];
    assert!(!session.disconnected);
    assert!(!session.loading());
    assert!(session.loaded());
}

#[test]
fn failed_older_page_preserves_the_cursor_for_retry() {
    let mut workspace = Workspace::default();
    let (index, request, summary) = begin(&mut workspace);
    workspace.apply(Event::Completed(
        request,
        Ok(Reply::Resumed(summary, history(false))),
    ));
    workspace.commands();

    workspace.older(index);
    let (request, command) = workspace.commands().pop().unwrap();
    assert!(matches!(command, Command::History(_, Some(cursor)) if cursor == "older"));
    assert!(workspace.sessions[index].loaded());
    assert!(workspace.sessions[index].loading());
    workspace.apply(Event::Completed(request, Err("page unavailable".into())));
    assert!(!workspace.sessions[index].loading());
    assert_eq!(workspace.sessions[index].next_cursor(), Some("older"));

    workspace.older(index);
    let command = workspace.commands().pop().unwrap().1;
    assert!(matches!(command, Command::History(_, Some(cursor)) if cursor == "older"));
}

#[test]
fn bundled_refresh_preserves_exhausted_pagination_and_restores_failure_notice() {
    let mut workspace = Workspace::default();
    let (index, request, summary) = begin(&mut workspace);
    workspace.apply(Event::Completed(
        request,
        Ok(Reply::Resumed(summary.clone(), history(false))),
    ));
    workspace.commands();
    workspace.older(index);
    let (older, command) = workspace.commands().pop().unwrap();
    assert!(matches!(command, Command::History(_, Some(cursor)) if cursor == "older"));
    workspace.apply(Event::Completed(older, Ok(page(Vec::new(), false))));
    assert!(workspace.sessions[index].next_cursor().is_none());
    workspace.refresh(index);
    let refresh = workspace.commands().pop().unwrap().0;
    let mut page = history(false);
    page.data.push(json!({"id":"idle", "type":"idle", "outcome":"failed", "time":{"created":123}, "error":{"message":"Provider failure"}}));
    workspace.apply(Event::Completed(refresh, Ok(Reply::Resumed(summary, page))));
    let session = &workspace.sessions[index];
    assert!(session.next_cursor().is_none());
    assert!(session.queue_paused);
    assert_eq!(
        session.conversation.failure.notice().unwrap().message,
        "Provider failure"
    );
}
