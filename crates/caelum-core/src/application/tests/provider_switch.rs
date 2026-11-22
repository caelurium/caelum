use super::*;

fn previous(app: &mut Workspace) -> (usize, Summary) {
    let old = summary(scope(Provider::OpenCode, "/project"), "original");
    let i = app.new_session(old.key.scope.clone());
    let s = &mut app.sessions[i];
    s.remote_id = Some(old.key.id.clone());
    s.title = "Investigate the timeout".into();
    s.draft = "What changed?".into();
    s.reading = 114.;
    s.conversation
        .history(Provider::OpenCode, &messages(), false);
    (i, old)
}

fn messages() -> Vec<serde_json::Value> {
    vec![
        json!({"id":"question","type":"user","text":"Which test fails?"}),
        json!({"id":"answer","type":"assistant","content":[{"type":"text","text":"The timeout test."}]}),
    ]
}

fn request(app: &mut Workspace, predicate: impl Fn(&Command) -> bool) -> u64 {
    app.commands()
        .into_iter()
        .find(|(_, c)| predicate(c))
        .unwrap()
        .0
}

#[test]
fn next_turn_changes_backend_in_place_and_delivers_context_until_admitted() {
    let mut app = Workspace::default();
    let (i, old) = previous(&mut app);
    let scope = scope(Provider::Codex, "/project");
    let listing = discover_session_list(&mut app, &old.key.scope, false);
    app.sessions[i].conversation.set_working(true);
    assert!(!app.switch_provider(i, Provider::Codex));
    app.sessions[i].conversation.settle();
    assert!(app.switch_provider(i, Provider::Codex));
    app.apply(Event::Completed(listing, Ok(Reply::Sessions(vec![old]))));
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(app.selected, Some(i));
    let s = &app.sessions[i];
    assert!(s.key().is_none());
    assert_eq!(s.draft, "What changed?");
    assert_eq!(s.reading, 114.);
    assert_eq!(s.conversation.entries.len(), 2);
    assert_eq!(
        s.previous_scope(&s.conversation.entries[1])
            .unwrap()
            .provider,
        Provider::OpenCode
    );
    assert!(app.send(i, "What changed?".into()));
    let start = request(
        &mut app,
        |c| matches!(c, Command::Start(owner, _) if owner == &scope),
    );
    app.apply(Event::Completed(
        start,
        Ok(Reply::Session(summary(scope.clone(), "new"))),
    ));
    let first = |c: &Command| matches!(c, Command::Send(owner, text, _) if owner.scope == scope && text.contains("The timeout test.") && text.ends_with("What changed?"));
    let send = request(&mut app, first);
    app.apply(Event::Completed(
        send,
        Err("Disconnected before admission".into()),
    ));
    assert!(app.send(i, "What changed?".into()));
    let commands = app.commands();
    let (send, Command::Send(_, envelope, _)) = commands.iter().find(|(_, c)| first(c)).unwrap()
    else {
        unreachable!()
    };
    app.apply(Event::Completed(*send, Ok(Reply::Done)));
    app.apply(codex("/project", "new", "item/completed", json!({"item":{"id":"question","type":"userMessage","content":[{"type":"text","text":envelope}]}})));
    let s = &mut app.sessions[i];
    assert_eq!(s.title, "Investigate the timeout");
    assert_eq!(s.conversation.entries.len(), 3);
    assert_eq!(s.conversation.entries[2].text, "What changed?");
    s.conversation.settle();
    assert!(app.send(i, "Next".into()));
    request(
        &mut app,
        |c| matches!(c, Command::Send(_, text, _) if text == "Next"),
    );
}

#[test]
fn restart_restores_old_transcript_from_its_original_provider_without_another_row() {
    let mut original = Workspace::default();
    let (i, old) = previous(&mut original);
    assert!(original.switch_provider(i, Provider::Antigravity));
    let snapshot =
        serde_json::from_slice(&serde_json::to_vec(&original.snapshot()).unwrap()).unwrap();
    let mut restored = Workspace::default();
    restored.enable_store("unused-restore-test.json".into());
    let load = request(&mut restored, |c| matches!(c, Command::Load(_)));
    restored.apply(Event::Completed(
        load,
        Ok(Reply::Snapshot(Some(Box::new(snapshot)))),
    ));
    let mut effects = restored.commands();
    if let Some((save, _)) = effects.iter().find(|(_, c)| matches!(c, Command::Save(..))) {
        restored.apply(Event::Completed(*save, Ok(Reply::Done)));
        effects.extend(restored.commands());
    }
    let read = effects
        .into_iter()
        .find(|(_, c)| matches!(c, Command::History(key, None) if key == &old.key))
        .unwrap()
        .0;
    let Reply::History(mut latest) = page(messages(), false) else {
        unreachable!()
    };
    latest.next_cursor = Some("older".into());
    restored.apply(Event::Completed(read, Ok(Reply::History(latest))));
    let session = &restored.sessions[i];
    assert_eq!(restored.selected, Some(i));
    assert_eq!(restored.sessions.len(), 1);
    assert_eq!(session.scope.provider, Provider::Antigravity);
    assert_eq!(session.conversation.entries[1].text, "The timeout test.");
    assert_eq!(
        session.previous_scope(&session.conversation.entries[1]),
        Some(&old.key.scope)
    );
    assert_eq!(session.draft, "What changed?");
    restored.older(i);
    let older = request(
        &mut restored,
        |c| matches!(c, Command::History(key, Some(cursor)) if key == &old.key && cursor == "older"),
    );
    restored.apply(Event::Completed(
        older,
        Ok(page(
            vec![
                json!({"id":"first","type":"user","text":"Set up the test."}),
                messages()[0].clone(),
            ],
            false,
        )),
    ));
    assert_eq!(restored.sessions[i].conversation.entries.len(), 3);
    assert_eq!(
        restored.sessions[i].conversation.entries[0].text,
        "Set up the test."
    );
    assert_eq!(restored.sessions[i].next_cursor(), None);
}

#[test]
fn switching_back_keeps_both_providers_turns_and_their_row_identity() {
    let mut app = Workspace::default();
    let (i, _) = previous(&mut app);
    let original_row = app.sessions[i].conversation.entries[0].id.clone();
    assert!(app.switch_provider(i, Provider::Antigravity));
    let session = &mut app.sessions[i];
    session.remote_id = Some("second".into());
    session.conversation.history(
        Provider::Antigravity,
        &[json!({"id":original_row, "type":"assistant", "text":"The retry fixed it."})],
        false,
    );
    let second_row = session
        .transcript_id(&session.conversation.entries[2])
        .into_owned();
    assert_ne!(second_row, original_row);
    assert!(app.switch_provider(i, Provider::OpenCode));
    let s = &app.sessions[i];
    assert_eq!(s.transcript_id(&s.conversation.entries[0]), original_row);
    assert_eq!(s.transcript_id(&s.conversation.entries[2]), second_row);
    assert_eq!(app.sessions.len(), 1);
    assert!(app.send(i, "Summarize".into()));
    let start = request(&mut app, |c| matches!(c, Command::Start(..)));
    app.apply(Event::Completed(
        start,
        Ok(Reply::Session(summary(
            scope(Provider::OpenCode, "/project"),
            "third",
        ))),
    ));
    request(
        &mut app,
        |c| matches!(c, Command::Send(key, text, _) if key.id == "third" && text.contains("The timeout test.") && text.contains("The retry fixed it.") && text.ends_with("Summarize")),
    );
}
