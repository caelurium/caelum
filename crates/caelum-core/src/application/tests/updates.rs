use super::*;

#[test]
fn restart_requires_the_latest_drafts_attachments_and_queue_to_be_durable() {
    let mut app = Workspace::default();
    let index = app.new_session(scope(Provider::Codex, "/project"));
    app.enable_store("/unused-in-reducer-test".into());
    assert!(!app.saved_for_restart());
    let (load, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(load, Ok(Reply::Snapshot(None))));
    app.sessions[index].draft = "first draft".into();
    let (first, _) = app.commands().pop().unwrap();
    app.sessions[index].draft = "latest café draft".into();
    app.attach(
        index,
        Attachment::new("file.rs".into(), "captured text".into(), None, false),
    );
    app.sessions[index].queue.push(QueuedPrompt {
        provider_command: false,
        text: "queued work".into(),
        options: Default::default(),
    });
    app.sessions[index].queue_paused = true;
    app.apply(Event::Completed(first, Ok(Reply::Done)));
    assert!(!app.saved_for_restart());
    let (second, command) = app.commands().pop().unwrap();
    let Command::Save(_, snapshot) = command else {
        panic!("expected save");
    };
    assert_eq!(snapshot.sessions[index].draft, "latest café draft");
    assert_eq!(snapshot.sessions[index].attachments.len(), 1);
    assert_eq!(snapshot.sessions[index].queue.len(), 1);
    app.apply(Event::Completed(second, Err("disk full".into())));
    assert!(!app.saved_for_restart());
    app.task(Task::Updates);
    assert!(app
        .commands()
        .iter()
        .any(|(_, command)| matches!(command, Command::Task(Task::Updates))));
    app.retry_save();
    let (saved, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(saved, Ok(Reply::Done)));
    assert!(app.saved_for_restart());
    app.sessions[index].attaching = 1;
    assert!(
        !app.saved_for_restart(),
        "pending captured attachments must finish first"
    );
    app.sessions[index].attaching = 0;
    app.home.attaching = 1;
    assert!(!app.saved_for_restart());
    app.home.attaching = 0;
    app.sessions[index].draft.push('!');
    assert!(!app.saved_for_restart());
}
