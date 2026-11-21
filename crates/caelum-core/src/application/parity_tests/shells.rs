use super::*;

fn completion(command: &str, output: &str, exit: i64) -> serde_json::Value {
    json!({
        "id": "msg_finished", "type": "synthetic", "time": {"created": 2500},
        "description": command,
        "metadata": {"source": "shell", "shellID": "sh_build", "state": "completed", "exit": exit, "truncated": false},
        "text": format!("<shell id=\"sh_build\" state=\"completed\" command=\"{command}\">\n{output}\n</shell>")
    })
}

#[test]
fn background_shell_completion_renders_output_live_and_after_history_refresh() {
    let command = "printf '%s\\n' \"café > log\"\ncargo test";
    let output = "  café output\n<shell>literal output</shell>\n";
    let message = completion(command, output, 0);
    let mut live = Conversation::default();
    let mut event = message.clone();
    event["_event_id"] = "evt_finished".into();
    event["_created"] = 2500.into();
    live.apply(Provider::OpenCode, "session.synthetic", &event);
    let mut restored = Conversation::default();
    restored.history(Provider::OpenCode, std::slice::from_ref(&message), false);
    live.history(Provider::OpenCode, std::slice::from_ref(&message), false);
    for conversation in [&live, &restored] {
        assert_eq!(conversation.entries.len(), 1);
        let entry = &conversation.entries[0];
        assert_eq!(entry.id, "opencode-shell-sh_build");
        assert_eq!(entry.kind, EntryKind::Tool);
        assert!(entry.background && entry.complete);
        assert!(presentation::entry_visible(entry));
        let tool = presentation::Tool::from_entry(entry, 9000);
        assert_eq!(tool.summary, command);
        assert_eq!(tool.output, output);
        assert_eq!(tool.outcome, presentation::Outcome::Success);
        assert_eq!(tool.details, ["Background"]);
        assert!(tool.timing.is_empty());
    }
}

#[test]
fn shell_completion_keeps_failure_timing_and_cannot_be_reopened_by_a_late_start() {
    let mut conversation = Conversation::default();
    let start = json!({"shell": {"id": "sh_build", "status": "running", "command": "cargo test", "time": {"started": 1000}, "metadata": {"exit": null, "ended": null}}});
    conversation.apply(Provider::OpenCode, "session.shell.started", &start);
    let mut event = completion("cargo test", "test failed", 101);
    event["_event_id"] = "evt_finished".into();
    event["_created"] = 2500.into();
    conversation.apply(Provider::OpenCode, "session.synthetic", &event);
    conversation.apply(Provider::OpenCode, "session.shell.started", &start);
    conversation.history(Provider::OpenCode, &[event], false);
    assert_eq!(conversation.entries.len(), 1);
    let entry = &conversation.entries[0];
    assert!(entry.complete);
    let tool = presentation::Tool::from_entry(entry, 9000);
    assert_eq!(tool.output, "test failed");
    assert_eq!(tool.outcome, presentation::Outcome::Failure);
    assert_eq!(tool.details, ["Background", "Exit 101"]);
    assert_eq!(tool.timing, "1.5s");
    assert!(!conversation.working);
}

#[test]
fn native_shell_history_preserves_output_timing_and_terminal_outcomes() {
    use presentation::Outcome;

    for (status, exit, outcome) in [
        ("exited", json!(0), Outcome::Success),
        ("exited", json!(101), Outcome::Failure),
        ("timeout", json!(null), Outcome::Failure),
        ("killed", json!(null), Outcome::Interrupted),
    ] {
        let message = json!({
            "id": "msg_shell", "type": "shell", "shellID": "sh_build", "command": "cargo test",
            "time": {"created": 1000, "completed": 2500}, "status": status, "exit": exit,
            "output": "final output"
        });
        let mut conversation = Conversation::default();
        conversation.history(Provider::OpenCode, std::slice::from_ref(&message), false);

        let mut running = message;
        running["status"] = "running".into();
        running["time"]["completed"] = json!(null);
        running["exit"] = json!(null);
        running["output"] = "partial output".into();
        conversation.history(Provider::OpenCode, &[running], false);
        assert_eq!(conversation.entries.len(), 1);
        let entry = &conversation.entries[0];
        assert!(entry.complete);
        let tool = presentation::Tool::from_entry(entry, 9000);
        assert_eq!(tool.output, "final output");
        assert_eq!(tool.timing, "1.5s");
        assert_eq!(tool.outcome, outcome);
    }
}

#[test]
fn legacy_job_id_and_cancelled_commands_keep_their_result() {
    for shell_id in [json!(null), json!("")] {
        let mut message = completion("cargo test", "stopped", 0);
        message["metadata"]["exit"] = json!(null);
        message["metadata"]["shellID"] = shell_id;
        message["metadata"]["jobID"] = "sh_build".into();
        message["metadata"]["state"] = "cancelled".into();
        message["text"] =
            "<shell id=\"sh_build\" state=\"cancelled\" command=\"cargo test\">\nstopped\n</shell>"
                .into();
        let mut conversation = Conversation::default();
        conversation.apply(Provider::OpenCode, "session.synthetic", &message);
        conversation.history(Provider::OpenCode, &[message], false);
        assert_eq!(conversation.entries.len(), 1);
        let entry = &conversation.entries[0];
        assert_eq!(entry.id, "opencode-shell-sh_build");
        let tool = presentation::Tool::from_entry(entry, 9000);
        assert_eq!(tool.output, "stopped");
        assert_eq!(tool.outcome, presentation::Outcome::Interrupted);
    }
}

#[test]
fn shell_output_without_the_expected_envelope_is_preserved() {
    for output in [
        "unwrapped output\n",
        "<shell id=\"sh_build\" state=\"completed\" command=\"cargo test\">\npartial output",
    ] {
        let mut message = completion("cargo test", "", 0);
        message["text"] = output.into();
        let mut conversation = Conversation::default();
        conversation.apply(Provider::OpenCode, "session.synthetic", &message);
        assert_eq!(conversation.entries[0].output, output);
    }
}

#[test]
fn shell_markup_without_provider_metadata_is_not_a_command_result() {
    let mut message = completion("cargo test", "output", 0);
    message["metadata"] = json!({"source": "plugin"});
    let mut conversation = Conversation::default();
    conversation.history(Provider::OpenCode, std::slice::from_ref(&message), false);
    assert_eq!(conversation.entries[0].kind, EntryKind::Status);
    assert_eq!(conversation.entries[0].text, message["text"]);
    message["type"] = "user".into();
    conversation.history(Provider::OpenCode, std::slice::from_ref(&message), false);
    assert_eq!(conversation.entries[0].kind, EntryKind::User);
    assert_eq!(conversation.entries[0].text, message["text"]);
}

#[test]
fn synthetic_shell_completion_retires_only_its_owner_and_rejects_a_late_output_read() {
    let mut app = workspace();
    app.sessions[1].scope.provider = Provider::OpenCode;
    app.apply(opencode("session.shell.started", json!({
        "sessionID": "session-0", "shell": {"id": "sh_build", "status": "running", "command": "cargo test", "time": {"started": 1000}}
    })));
    app.commands();
    app.task(Task::Shell {
        project: "/project".into(),
        session: "session-0".into(),
        id: "sh_build".into(),
        stop: false,
    });
    let read = app
        .commands()
        .into_iter()
        .find(|(_, c)| matches!(c, Command::Task(Task::Shell { .. })))
        .unwrap()
        .0;
    app.select(1);
    let mut event = completion("cargo test", "final output", 0);
    event["sessionID"] = "session-0".into();
    event["_event_id"] = "evt_finished".into();
    app.apply(opencode("session.synthetic", event));
    app.apply(opencode("session.shell.started", json!({
        "sessionID": "session-0", "shell": {"id": "sh_build", "status": "running", "command": "cargo test", "time": {"started": 1000}}
    })));
    assert!(app.sessions[0].shells.is_empty());
    assert_eq!(app.selected, Some(1));
    assert!(app.sessions[1].conversation.entries.is_empty());
    app.apply(Event::Completed(
        read,
        Ok(Reply::Task(TaskReply::Shell("stale output".into()))),
    ));
    assert!(app.services.checkouts["/project"].shell_output.is_empty());
    assert_eq!(
        app.sessions[0].conversation.entries[0].output,
        "final output"
    );
}
