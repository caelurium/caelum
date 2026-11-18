use super::*;

fn parent(provider: Provider) -> (Workspace, SessionKey) {
    let mut app = Workspace::default();
    app.projects.push(ProjectInfo {
        path: "/project".into(),
        name: "Project".into(),
    });
    let i = app.new_session(scope(provider, "/project"));
    let s = &mut app.sessions[i];
    s.remote_id = Some("parent".into());
    s.draft = "Parent draft".into();
    s.reading = 92.0;
    s.attachments.push(Attachment::new(
        "example.rs".into(),
        "captured".into(),
        None,
        false,
    ));
    let source = s.key().unwrap();
    (app, source)
}

fn provider_page(app: &mut Workspace, records: Vec<serde_json::Value>, next: Option<&str>) {
    let id = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::History(..)))
        .unwrap()
        .0;
    let Reply::History(mut p) = page(records, false) else {
        unreachable!()
    };
    p.next_cursor = next.map(str::to_owned);
    app.apply(Event::Completed(id, Ok(Reply::History(p))));
}

fn create(app: &mut Workspace, source: &SessionKey, boundary: ForkBoundary) -> u64 {
    app.fork_at(&boundary);
    app.fork_at(&boundary);
    let mut requests = app.commands().into_iter().filter_map(|(id, c)| match c {
        Command::Fork(owner, b) => {
            assert_eq!(owner, *source);
            assert_eq!(b, boundary);
            Some(id)
        }
        _ => None,
    });
    let id = requests.next().unwrap();
    assert!(requests.next().is_none());
    id
}

#[test]
fn opencode_fork_across_a_page_break_keeps_the_full_selected_exchange_and_does_not_copy_drafts() {
    let (mut app, source) = parent(Provider::OpenCode);
    assert!(app.open_fork(0));
    provider_page(
        &mut app,
        vec![
            json!({"id":"msg_reply1","type":"assistant"}),
            json!({"id":"msg_2","type":"user","text":"Second"}),
            json!({"id":"msg_tail","type":"idle"}),
        ],
        Some("older"),
    );
    assert_eq!(
        app.fork_picker().unwrap().points[0].boundary,
        ForkBoundary::OpenCodeTip("msg_tail".into())
    );
    app.older_fork_points();
    provider_page(
        &mut app,
        vec![json!({"id":"msg_1","type":"user","text":"First"})],
        None,
    );
    let point = &app.fork_picker().unwrap().points[0];
    assert_eq!(point.label, "First");
    assert_eq!(point.boundary, ForkBoundary::OpenCodeBefore("msg_2".into()));
    let id = create(
        &mut app,
        &source,
        ForkBoundary::OpenCodeBefore("msg_2".into()),
    );
    app.apply(Event::Completed(
        id,
        Ok(Reply::Forked {
            source: source.clone(),
            child: summary(source.scope, "new-child"),
        }),
    ));
    assert_eq!(app.selected, Some(1));
    assert!(app.fork_picker().is_none());
    assert!(
        app.sessions[1].draft.is_empty()
            && app.sessions[1].attachments.is_empty()
            && app.sessions[1].reading < 0.0
    );
    assert_eq!(app.sessions[0].draft, "Parent draft");
    assert_eq!(app.sessions[0].attachments[0].content, "captured");
    assert_eq!(app.sessions[0].reading, 92.0);
    assert!(app
        .commands()
        .iter()
        .any(|(_, c)| matches!(c,Command::Resume(k) if k.id=="new-child")));
}

#[test]
fn codex_fork_reuses_a_discovered_child_and_does_not_navigate_after_dismissal_or_navigation() {
    for navigate in [true, false] {
        let (mut app, source) = parent(Provider::Codex);
        assert!(app.open_fork(0));
        provider_page(
            &mut app,
            vec![
                json!({"id":"turn","status":"completed","items":[{"type":"userMessage","id":"item","content":[{"type":"text","text":"Earlier"}]}]}),
                json!({"id":"pending","status":"inProgress","items":[]}),
            ],
            None,
        );
        let id = create(&mut app, &source, ForkBoundary::CodexTurn("turn".into()));
        let list = discover_session_list(&mut app, &source.scope, false);
        app.apply(Event::Completed(
            list,
            Ok(Reply::Sessions(vec![summary(
                source.scope.clone(),
                "child",
            )])),
        ));
        app.sessions[1].draft = "Already editing".into();
        if navigate {
            app.new_session(source.scope.clone());
        } else {
            app.dismiss_fork();
        }
        let count = app.sessions.len();
        let selection = app.selected;
        app.apply(Event::Completed(
            id,
            Ok(Reply::Forked {
                source: source.clone(),
                child: summary(source.scope, "child"),
            }),
        ));
        assert_eq!(app.selected, selection);
        assert_eq!(app.sessions.len(), count);
        assert_eq!(app.sessions[1].draft, "Already editing");
    }
}

#[test]
fn fork_checks_admission_again_and_failures_do_not_change_the_parent() {
    let (mut app, source) = parent(Provider::OpenCode);
    assert!(app.open_fork(0));
    provider_page(
        &mut app,
        vec![json!({"id":"msg_a","type":"user","text":"Prompt"})],
        None,
    );
    let bound = ForkBoundary::OpenCodeTip("msg_a".into());
    app.sessions[0].conversation.working = true;
    app.fork_at(&bound);
    assert!(app.fork_picker().unwrap().error.is_some());
    assert!(app.commands().is_empty());
    app.sessions[0].conversation.working = false;
    let id = create(&mut app, &source, bound.clone());
    app.apply(Event::Completed(id, Err("Provider unavailable".into())));
    assert_eq!(
        app.fork_picker().unwrap().error.as_deref(),
        Some("Provider unavailable")
    );
    let id = create(&mut app, &source, bound);
    app.apply(Event::Completed(
        id,
        Ok(Reply::Forked {
            source: source.clone(),
            child: summary(source.scope, "parent"),
        }),
    ));
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.sessions[0].draft, "Parent draft");
    assert!(app.fork_picker().unwrap().error.is_some());
}
