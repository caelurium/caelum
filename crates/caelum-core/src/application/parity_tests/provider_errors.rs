use super::*;

fn failed(at: u64) -> Event {
    opencode(
        "session.execution.failed",
        json!({
            "sessionID":"session-0", "_created":at,
            "error":{"type":"provider.transport", "status":200,
                "message":"Timed out waiting for response data"}
        }),
    )
}

#[test]
fn provider_failures_are_scoped_notices_and_clear_on_new_submission() {
    let mut app = workspace();
    let other = app.new_session(Scope {
        project: "/other".into(),
        provider: Provider::OpenCode,
    });
    app.sessions[other].remote_id = Some("session-0".into());
    app.sessions[0].draft = "Keep this draft".into();
    app.apply(failed(100));
    let session = &app.sessions[0];
    let notice = session.conversation.failure.notice().unwrap();
    assert_eq!(notice.message, "Timed out waiting for response data");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&notice.details).unwrap()["type"],
        "provider.transport"
    );
    assert!(session.conversation.entries.is_empty());
    assert!(session.unread);
    assert!(session.queue_paused);
    assert_eq!(session.draft, "Keep this draft");
    assert!(app.sessions[1].conversation.failure.notice().is_none());
    assert!(app.sessions[other].conversation.failure.notice().is_none());
    assert!(app.send(0, "Try again".into()));
    assert!(app.sessions[0].conversation.failure.notice().is_none());
    assert!(app.sessions[0].working());
    app.sessions[0].queue_paused = false;
    app.apply(failed(150));
    assert!(app.sessions[0].conversation.failure.notice().is_none());
    assert!(!app.sessions[0].queue_paused);
    assert!(app.sessions[0].working());
}

#[test]
fn dismissal_recovery_and_late_events_cannot_replace_a_newer_failure_or_turn() {
    let mut app = workspace();
    app.apply(failed(100));
    let old = app.sessions[0]
        .conversation
        .failure
        .notice()
        .unwrap()
        .generation;
    app.apply(failed(200));
    let failure = &mut app.sessions[0].conversation.failure;
    failure.dismiss(old);
    let current = failure.notice().unwrap().generation;
    failure.dismiss(current);
    failure.history(
        &[
            json!({"type":"idle","outcome":"failed","time":{"created":200},
        "error":{"message":"Timed out waiting for response data"}}),
        ],
        false,
    );
    assert!(failure.notice().is_none());
    app.apply(failed(300));
    app.apply(opencode(
        "session.execution.started",
        json!({"sessionID":"session-0", "_created":400}),
    ));
    app.sessions[0].queue_paused = false;
    app.apply(failed(350));
    assert!(app.sessions[0].conversation.failure.notice().is_none());
    assert!(app.sessions[0].working());
    assert!(!app.sessions[0].queue_paused);
    for (index, method) in [
        "session.retry.scheduled",
        "session.text.delta",
        "session.execution.succeeded",
    ]
    .into_iter()
    .enumerate()
    {
        app.sessions[0].invalidate_history();
        let at = 500 + index as u64 * 200;
        app.apply(failed(at));
        assert!(app.sessions[0].conversation.failure.notice().is_some());
        app.apply(opencode(
            method,
            json!({"sessionID":"session-0", "_created":at+100,
            "assistantMessageID":"reply", "delta":"Recovered"}),
        ));
        assert!(
            app.sessions[0].conversation.failure.notice().is_none(),
            "{method}"
        );
    }
    assert!(app.sessions[0]
        .conversation
        .entries
        .iter()
        .all(|e| e.kind() != EntryKind::Status));
}

#[test]
fn unsupported_capability_has_recovery_guidance_and_copyable_provider_details() {
    let mut app = workspace();
    app.apply(opencode("session.execution.failed", json!({
        "sessionID":"session-0", "error":{
            "type":"provider.invalid-request", "message":"Unsupported feature",
            "response":{"body":json!({"error":{"code":"subscription_sharing_unsupported_capability",
                "param":"input"},"status":400}).to_string()}
        }
    })));
    let notice = app.sessions[0].conversation.failure.notice().unwrap();
    assert!(notice
        .message
        .contains("model and effort selected before sending"));
    assert!(!notice.message.contains("response"));
    assert!(notice
        .details
        .contains("subscription_sharing_unsupported_capability"));
    app.apply(opencode(
        "session.execution.failed",
        json!({"sessionID":"session-0",
        "error":{"type":"provider.unknown","response":{"body":"<html>Bad gateway</html>"}}}),
    ));
    assert_eq!(
        app.sessions[0]
            .conversation
            .failure
            .notice()
            .unwrap()
            .message,
        "The provider could not finish this response."
    );
}

#[test]
fn current_history_restores_failures_but_an_obsolete_refresh_cannot_resurrect_them() {
    use caelum_protocol::SessionHistoryPage;
    let mut app = workspace();
    app.sessions[0].invalidate_history();
    let page = || {
        Reply::History(SessionHistoryPage {
            data: vec![
                json!({"id":"idle", "type":"idle", "outcome":"failed", "time":{"created":300},
            "error":{"message":"Timed out waiting for response data"}}),
            ],
            is_working: Some(false),
            next_cursor: None,
            model: None,
            effort: None,
            agent: None,
        })
    };
    let history = |app: &mut Workspace| {
        app.refresh(0);
        let (request, _) = app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Resume(_)))
            .unwrap();
        let key = app.sessions[0].key().unwrap();
        app.apply(Event::Completed(
            request,
            Ok(Reply::Session(Summary {
                key,
                title: "Example".into(),
                updated_at: None,
                working: false,
                options: Options::default(),
            })),
        ));
        app.commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::History(..)))
            .unwrap()
            .0
    };
    let request = history(&mut app);
    app.apply(Event::Completed(request, Ok(page())));
    assert!(app.sessions[0].queue_paused);
    let generation = app.sessions[0]
        .conversation
        .failure
        .notice()
        .unwrap()
        .generation;
    app.sessions[0].conversation.failure.dismiss(generation);
    let request = history(&mut app);
    app.apply(Event::Completed(request, Ok(page())));
    assert!(app.sessions[0].conversation.failure.notice().is_none());
    let request = history(&mut app);
    app.apply(opencode(
        "session.execution.started",
        json!({"sessionID":"session-0", "_created":400}),
    ));
    app.apply(Event::Completed(request, Ok(page())));
    assert!(app.sessions[0].conversation.failure.notice().is_none());
    assert!(app.sessions[0].working());
}
