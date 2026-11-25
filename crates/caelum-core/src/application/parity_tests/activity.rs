use super::*;

#[test]
fn completed_outputs_keep_the_used_agent_and_full_turn_duration() {
    use caelum_protocol::CodexEvent;

    let mut app = workspace();
    app.sessions[0].options.agent = Some("build".into());
    assert!(app.send(0, "OpenCode prompt".into()));
    app.sessions[0].options.agent = Some("plan".into());
    for (method, data) in [
        (
            "session.execution.started",
            json!({"sessionID":"session-0", "_created":1000}),
        ),
        (
            "session.text.started",
            json!({"sessionID":"session-0", "assistantMessageID":"message-0", "ordinal":0, "agent":"build", "_created":1200}),
        ),
        (
            "session.text.delta",
            json!({"sessionID":"session-0", "assistantMessageID":"message-0", "ordinal":0, "delta":"OpenCode answer"}),
        ),
        (
            "session.text.ended",
            json!({"sessionID":"session-0", "assistantMessageID":"message-0", "ordinal":0, "text":"OpenCode answer", "agent":"build", "_created":4000}),
        ),
        (
            "session.execution.succeeded",
            json!({"sessionID":"session-0", "_created":5000}),
        ),
    ] {
        app.apply(opencode(method, data));
    }
    let open_code = app.sessions[0]
        .conversation
        .entries
        .iter()
        .find(|entry| entry.kind() == crate::application::EntryKind::Assistant)
        .unwrap();
    assert_eq!(open_code.metadata["agent"], "build");
    assert_eq!(open_code.metadata["outputDurationMs"], 4000);

    app.sessions[1].options.agent = Some("plan".into());
    assert!(app.send(1, "Codex prompt".into()));
    app.sessions[1].options.agent = Some("build".into());
    let codex = |method: &str, mut params: serde_json::Value| {
        params["threadId"] = "session-1".into();
        Event::Codex(CodexEvent {
            project_path: "/project".into(),
            method: method.into(),
            params,
            request_id: None,
        })
    };
    for (method, params) in [
        (
            "turn/started",
            json!({"turn":{"id":"turn-1"}, "_created":10000}),
        ),
        (
            "item/started",
            json!({"item":{"id":"answer-1", "type":"agentMessage", "status":"inProgress"}, "_created":10200}),
        ),
        (
            "item/completed",
            json!({"item":{"id":"answer-1", "type":"agentMessage", "status":"completed", "text":"Codex answer"}, "_created":13000}),
        ),
        (
            "turn/completed",
            json!({"turn":{"id":"turn-1", "status":"completed"}, "_created":15000}),
        ),
    ] {
        app.apply(codex(method, params));
    }
    let codex = app.sessions[1]
        .conversation
        .entries
        .iter()
        .find(|entry| entry.kind() == crate::application::EntryKind::Assistant)
        .unwrap();
    assert_eq!(codex.metadata["agent"], "plan");
    assert_eq!(codex.metadata["outputDurationMs"], 5000);
}

#[test]
fn provider_history_retains_output_agent_and_timing_when_available() {
    let mut open_code = crate::application::Conversation::default();
    open_code.history(
        Provider::OpenCode,
        &[
            json!({"id":"prompt-1", "type":"user", "text":"Review", "time":{"created":500}}),
            json!({
                "id":"message-1", "type":"assistant", "agent":"review",
                "time":{"created":1000, "completed":6500},
                "content":[{"type":"text", "text":"Reviewed"}]
            }),
        ],
        false,
    );
    let entry = &open_code.entries[1];
    assert_eq!(entry.metadata["agent"], "review");
    assert_eq!(entry.metadata["outputDurationMs"], 6000);

    let mut codex = crate::application::Conversation::default();
    codex.history(
        Provider::Codex,
        &[json!({
            "id":"turn-1", "status":"completed", "agent":"planner",
            "createdAt":1700000000, "completedAt":1700000005,
            "items":[{"id":"message-1", "type":"agentMessage", "text":"Planned"}]
        })],
        false,
    );
    let entry = &codex.entries[0];
    assert_eq!(entry.metadata["agent"], "planner");
    assert_eq!(entry.metadata["outputDurationMs"], 5000);
}

#[test]
fn unread_and_notifications_follow_visibility_and_do_not_replay_completed_activity() {
    let mut app = workspace();
    app.preferences.sound_volume = 0;
    app.apply(opencode(
        "session.text.delta",
        json!({"sessionID":"session-0","assistantMessageID":"answer","delta":"hello"}),
    ));
    assert!(app.acknowledge());
    assert!(!app.sessions[0].unread);
    app.conversation_hidden = true;
    let mut ended = opencode(
        "session.text.ended",
        json!({"sessionID":"session-0","assistantMessageID":"answer","text":"hello world"}),
    );
    if let Event::OpenCode(event) = &mut ended {
        event.id = Some("evt-ended".into());
    }
    app.apply(ended);
    assert!(!app.acknowledge());
    assert!(app.sessions[0].unread);
    app.apply(opencode(
        "session.execution.succeeded",
        json!({"sessionID":"session-0"}),
    ));
    let commands = app.commands();
    assert_eq!(
        commands
            .iter()
            .filter(|(_, c)| matches!(c, Command::Notify(..)))
            .count(),
        1
    );
    app.apply(opencode(
        "session.execution.succeeded",
        json!({"sessionID":"session-0"}),
    ));
    assert!(!app
        .commands()
        .iter()
        .any(|(_, c)| matches!(c, Command::Notify(..))));
    for (request, command) in commands {
        if matches!(command, Command::Resume(_)) {
            app.apply(Event::Completed(
                request,
                Err("offline test fixture".into()),
            ));
        }
    }
    app.conversation_hidden = false;
    app.sessions[0].reading = 300.0;
    assert!(!app.acknowledge());
    app.sessions[0].reading = -1.0;
    assert!(app.acknowledge());
    let mut duplicate = opencode(
        "session.text.ended",
        json!({"sessionID":"session-0","assistantMessageID":"answer","text":"hello world"}),
    );
    if let Event::OpenCode(event) = &mut duplicate {
        event.id = Some("evt-ended".into());
    }
    app.apply(duplicate);
    assert!(!app.sessions[0].unread);
    app.window_background = true;
    app.apply(opencode(
        "session.text.delta",
        json!({"sessionID":"session-0","assistantMessageID":"next","delta":"new"}),
    ));
    assert!(!app.acknowledge());
    assert!(app.snapshot().sessions[0].unread);
    let prompt = caelum_protocol::ApprovalPrompt {
        attempt: "fixture".into(),
        repeatable: None,
        provider: "codex".into(),
        project_path: "/project".into(),
        session_id: Some("session-1".into()),
        request_id: "request".into(),
        summary: "Permission".into(),
        description: String::new(),
    };
    app.apply(Event::Approvals(vec![prompt.clone()]));
    app.apply(Event::Approvals(vec![prompt]));
    assert_eq!(
        app.commands()
            .iter()
            .filter(|(_, c)| matches!(c, Command::Notify(..)))
            .count(),
        1
    );
}

#[test]
fn session_activity_survives_gaps_and_obeys_waiting_retry_and_terminal_states() {
    use presentation::{activity, ActivityKind as A};
    let mut app = workspace();
    assert!(app.send(1, "Work on this".into()));
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Working);
    let send = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(send, Ok(Reply::Done)));
    app.sessions[1].conversation.apply(
        Provider::Codex,
        "item/reasoning/summaryTextDelta",
        &json!({"itemId":"thought","delta":"Think"}),
    );
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Thinking);
    app.sessions[1].conversation.apply(
        Provider::Codex,
        "item/completed",
        &json!({"item":{"id":"thought","type":"reasoning","summary":[{"text":"Think"}]}}),
    );
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Working);
    app.sessions[1]
        .conversation
        .apply(Provider::OpenCode, "session.retry.scheduled", &json!({}));
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Retrying);
    app.sessions[1].conversation.apply(
        Provider::Codex,
        "item/started",
        &json!({"item":{"id":"tool","type":"commandExecution","command":"cargo test"}}),
    );
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Tool);
    assert_eq!(activity(&app.sessions[1], true).unwrap().kind, A::Input);
    app.cancel(1);
    assert_eq!(activity(&app.sessions[1], false).unwrap().kind, A::Stopping);
    let stop = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(stop, Ok(Reply::Done)));
    assert!(activity(&app.sessions[1], false).is_none());
    app.sessions[1].shells.push(
        serde_json::from_value(json!({"id":"shell","command":"cargo test","startedAt":1})).unwrap(),
    );
    assert_eq!(
        activity(&app.sessions[1], false).unwrap().kind,
        A::Background
    );
    app.sessions[1].disconnected = true;
    assert!(activity(&app.sessions[1], false).is_none());
    assert!(activity(&app.sessions[0], false).is_none());
}

#[test]
fn refreshing_statistics_preserves_the_same_filter_cache_and_rejects_old_ranges() {
    let mut app = workspace();
    let task = Task::Stats {
        project: "/project".into(),
        filter: caelum_protocol::OpenCodeStatsFilter {
            from: Some(100),
            ..Default::default()
        },
    };
    app.task(task.clone());
    let (old, _) = app.commands().pop().unwrap();
    let other = Task::Stats {
        project: "/project".into(),
        filter: caelum_protocol::OpenCodeStatsFilter {
            from: Some(200),
            ..Default::default()
        },
    };
    app.task(other.clone());
    let (new, _) = app.commands().pop().unwrap();
    app.apply(Event::Completed(old, Err("old failure".into())));
    assert!(app.services.busy(&other));
    assert!(app.services.error(&other).is_none());
    app.apply(Event::Completed(new, Err("current failure".into())));
    assert_eq!(app.services.error(&other), Some("current failure"));
}

#[test]
fn antigravity_turn_yielded_keeps_running_command_activity_until_completion() {
    use caelum_protocol::AntigravityEvent;
    use presentation::{activity, ActivityKind as A};

    let mut app = workspace();
    let index = app.new_session(Scope {
        project: "/project".into(),
        provider: Provider::Antigravity,
    });
    app.sessions[index].remote_id = Some("ag-1".into());
    app.selected = Some(0);
    app.commands();

    let ag = |method: &str, params: serde_json::Value| {
        Event::Antigravity(AntigravityEvent {
            project_path: "/project".into(),
            session_id: Some("ag-1".into()),
            method: method.into(),
            params,
        })
    };

    app.apply(ag("antigravity/turnStarted", json!({"_created": 1000})));
    app.apply(ag(
        "tool_call",
        json!({
            "toolCallId": "call-bg",
            "name": "run_command",
            "kind": "execute",
            "title": "make check",
            "status": "in_progress",
            "rawInput": {"CommandLine": "make check"}
        }),
    ));
    app.apply(ag(
        "agent_message_chunk",
        json!({"content": {"text": "Running make check. I'll update you once it finishes."}}),
    ));
    assert_eq!(
        activity(&app.sessions[index], false).unwrap().kind,
        A::Responding
    );

    app.apply(ag("antigravity/turnYielded", json!({})));
    assert!(app.sessions[index].working());
    let current = activity(&app.sessions[index], false).expect("still active");
    assert_eq!(current.kind, A::Tool);
    assert_eq!(current.label, "Running a command…");
    assert!(
        !app.commands()
            .iter()
            .any(|(_, c)| matches!(c, Command::Notify(..))),
        "must not fire Session finished notification while background task is running"
    );

    app.apply(ag(
        "tool_call_update",
        json!({"toolCallId": "call-bg", "status": "completed"}),
    ));
    assert_eq!(
        activity(&app.sessions[index], false).unwrap().kind,
        A::Working
    );

    app.apply(ag(
        "agent_message_chunk",
        json!({"content": {"text": "\n\nAll checks passed."}}),
    ));
    assert_eq!(
        activity(&app.sessions[index], false).unwrap().kind,
        A::Responding
    );

    app.apply(ag(
        "antigravity/turnCompleted",
        json!({"stopReason": "end_turn", "_created": 5000}),
    ));
    assert!(!app.sessions[index].working());
    assert!(activity(&app.sessions[index], false).is_none());
    assert!(app
        .commands()
        .iter()
        .any(|(_, c)| matches!(c, Command::Notify(..))));
}
