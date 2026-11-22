use super::*;
use crate::application::subagents::Subagent;

fn launch(id: &str, at: u64) -> serde_json::Value {
    json!({"id": format!("msg_{id}"), "type": "assistant", "time": {"created": at, "completed": at + 100}, "content": [{
        "type": "tool", "id": id, "name": "subagent", "time": {"created": at, "completed": at + 100},
        "state": {"status": "completed", "input": {"agent": "explore", "description": "Map request flow", "prompt": "Trace the café request path.", "background": true},
        "metadata": {"sessionID": "ses_child", "status": "running"}, "content": [{"type": "text", "text": "Subagent running in the background"}]}
    }]})
}

fn result(id: &str, at: u64, state: &str) -> serde_json::Value {
    json!({"id": id, "type": "synthetic", "time": {"created": at}, "description": "Map request flow",
        "metadata": {"source": "subagent", "childID": "ses_child", "agent": "explore", "state": state},
        "text": format!("<subagent sessionID=\"ses_child\" state=\"{state}\" description=\"Map request flow\">\nFound the café handler.\n</subagent>")})
}

#[test]
fn background_subagents_stay_running_until_the_report_and_reconcile_with_history() {
    let start = launch("task", 1000);
    let done = result("msg_done", 2500, "completed");
    let mut live = Conversation::default();
    live.apply(
        Provider::OpenCode,
        "session.tool.input.started",
        &json!({"id":"task", "name":"subagent", "_created":1000}),
    );
    let part = &start["content"][0]["state"];
    live.apply(
        Provider::OpenCode,
        "session.tool.called",
        &json!({"id":"task", "input":part["input"]}),
    );
    live.apply(
        Provider::OpenCode,
        "session.tool.progress",
        &json!({"id":"task", "metadata":part["metadata"]}),
    );
    let success = json!({"id":"task", "metadata":part["metadata"], "content":part["content"], "_created":1100});
    live.apply(Provider::OpenCode, "session.tool.success", &success);
    live.settle();
    assert!(!live.entries[0].complete());
    assert!(live.entries[0].background());
    assert!(live.entries[0].output.is_empty());
    assert_eq!(
        presentation::Tool::from_entry(&live.entries[0], 2000).timing,
        "1.0s"
    );
    live.apply(Provider::OpenCode, "session.synthetic", &done);
    live.apply(Provider::OpenCode, "session.tool.success", &success);
    live.history(Provider::OpenCode, std::slice::from_ref(&start), false);
    let mut restored = Conversation::default();
    restored.history(Provider::OpenCode, &[start.clone(), done.clone()], false);
    live.history(Provider::OpenCode, &[start, done], false);
    for conversation in [&live, &restored] {
        assert_eq!(conversation.entries.len(), 1);
        let entry = &conversation.entries[0];
        assert_eq!(entry.id, "opencode-tool-task");
        assert!(entry.complete());
        let tool = presentation::Tool::from_entry(entry, 9000);
        assert_eq!(tool.output, "Found the café handler.");
        assert_eq!(tool.outcome, presentation::Outcome::Success);
        assert_eq!(tool.timing, "1.5s");
        let task = Subagent::from_entry(entry).unwrap();
        assert_eq!(task.agent, "explore");
        assert_eq!(task.description, "Map request flow");
        assert_eq!(task.prompt, "Trace the café request path.");
        assert_eq!(task.session_id.as_deref(), Some("ses_child"));
    }
}

#[test]
fn completion_only_pages_keep_results_and_merge_when_older_launches_arrive() {
    for (state, outcome) in [
        ("completed", presentation::Outcome::Success),
        ("error", presentation::Outcome::Failure),
        ("cancelled", presentation::Outcome::Interrupted),
    ] {
        let mut conversation = Conversation::default();
        let done = result("msg_done", 2500, state);
        conversation.history(Provider::OpenCode, &[done], false);
        let entry = &conversation.entries[0];
        assert_eq!(entry.kind(), EntryKind::Tool);
        assert!(Subagent::from_entry(entry).is_some());
        let tool = presentation::Tool::from_entry(entry, 9000);
        assert_eq!(tool.output, "Found the café handler.");
        assert_eq!(tool.outcome, outcome);
        assert!(tool.timing.is_empty());
        conversation.history(Provider::OpenCode, &[launch("task", 1000)], true);
        assert_eq!(conversation.entries.len(), 1);
        assert_eq!(conversation.entries[0].id, "opencode-tool-task");
        assert_eq!(
            presentation::Tool::from_entry(&conversation.entries[0], 9000).outcome,
            outcome
        );
    }
}

#[test]
fn a_late_report_does_not_finish_a_newer_round_in_the_same_child() {
    let mut conversation = Conversation::default();
    let first = result("msg_first", 2000, "completed");
    conversation.history(
        Provider::OpenCode,
        &[launch("first", 1000), first.clone(), launch("next", 3000)],
        false,
    );
    conversation.apply(Provider::OpenCode, "session.synthetic", &first);
    assert_eq!(conversation.entries.len(), 2);
    assert!(conversation.entries[0].complete());
    assert!(!conversation.entries[1].complete());
    let mut second = result("msg_second", 5000, "error");
    second["text"] = "Second round failed".into();
    conversation.apply(Provider::OpenCode, "session.synthetic", &second);
    assert_eq!(conversation.entries.len(), 2);
    assert!(conversation.entries[1].complete());
    assert_eq!(conversation.entries[0].output, "Found the café handler.");
    assert_eq!(conversation.entries[1].output, "Second round failed");
    assert_eq!(
        presentation::Tool::from_entry(&conversation.entries[1], 9000).timing,
        "2.0s"
    );
}

#[test]
fn followups_joining_a_running_child_finish_together_without_repeating_the_report() {
    let mut conversation = Conversation::default();
    conversation.history(
        Provider::OpenCode,
        &[launch("first", 1000), launch("followup", 1500)],
        false,
    );
    assert_eq!(
        crate::application::subagents::running(&conversation.entries),
        1
    );
    conversation.apply(
        Provider::OpenCode,
        "session.synthetic",
        &result("msg_done", 2500, "completed"),
    );
    assert_eq!(
        crate::application::subagents::running(&conversation.entries),
        0
    );
    assert_eq!(conversation.entries.len(), 2);
    assert!(conversation.entries[0].output.is_empty());
    assert_eq!(conversation.entries[1].output, "Found the café handler.");
}

#[test]
fn paged_handoffs_finish_after_the_report_has_already_joined_another_task() {
    for order in [["followup", "first"], ["first", "followup"]] {
        let mut conversation = Conversation::default();
        conversation.history(
            Provider::OpenCode,
            &[result("msg_done", 2500, "completed")],
            false,
        );
        for id in order {
            conversation.history(
                Provider::OpenCode,
                &[launch(id, if id == "first" { 1000 } else { 1500 })],
                true,
            );
        }
        assert_eq!(
            crate::application::subagents::running(&conversation.entries),
            0
        );
        assert_eq!(conversation.entries.len(), 2);
        for entry in &conversation.entries {
            assert!(entry.complete());
            assert_eq!(entry.metadata["ended"], 2500);
            assert_eq!(
                entry.output,
                if entry.id == "opencode-tool-followup" {
                    "Found the café handler."
                } else {
                    ""
                }
            );
        }
    }
}

#[test]
fn reports_arriving_in_reverse_order_recover_each_rounds_own_outcome() {
    let mut conversation = Conversation::default();
    conversation.history(
        Provider::OpenCode,
        &[
            launch("first", 1000),
            launch("joined", 1500),
            launch("next", 3000),
        ],
        false,
    );
    let mut newer = result("msg_newer", 5000, "error");
    newer["text"] = "Next round failed".into();
    conversation.apply(Provider::OpenCode, "session.synthetic", &newer);
    conversation.apply(
        Provider::OpenCode,
        "session.synthetic",
        &result("msg_first", 2000, "completed"),
    );
    assert_eq!(conversation.entries.len(), 3);
    assert_eq!(
        crate::application::subagents::running(&conversation.entries),
        0
    );
    assert_eq!(conversation.entries[0].status(), "completed");
    assert_eq!(conversation.entries[0].metadata["ended"], 2000);
    assert!(conversation.entries[0].output.is_empty());
    assert_eq!(conversation.entries[1].output, "Found the café handler.");
    assert_eq!(conversation.entries[2].status(), "error");
    assert_eq!(conversation.entries[2].output, "Next round failed");
}

#[test]
fn foreground_results_keep_output_and_unrelated_markup_stays_literal() {
    let mut message = launch("task", 1000);
    message["content"][0]["state"]["metadata"]["status"] = "completed".into();
    message["content"][0]["state"]["content"][0]["text"] =
        "<subagent sessionID=\"ses_child\" state=\"completed\">\nResult\n</subagent>".into();
    let mut conversation = Conversation::default();
    conversation.history(Provider::OpenCode, &[message], false);
    assert!(conversation.entries[0].complete());
    assert_eq!(conversation.entries[0].output, "Result");
    let mut literal = result("msg_literal", 3000, "completed");
    literal["metadata"]["source"] = "plugin".into();
    conversation.history(Provider::OpenCode, std::slice::from_ref(&literal), false);
    assert_eq!(conversation.entries[1].kind(), EntryKind::Status);
    assert_eq!(conversation.entries[1].text, literal["text"]);
}

#[test]
fn opening_a_subagent_reuses_its_scoped_session_without_changing_parent_drafts() {
    let mut app = workspace();
    app.sessions[0].draft = "Parent draft".into();
    app.sessions[0]
        .conversation
        .history(Provider::OpenCode, &[launch("task", 1000)], false);

    app.sessions[1].scope = Scope {
        project: "/other".into(),
        provider: Provider::OpenCode,
    };
    app.sessions[1].remote_id = Some("ses_child".into());
    app.select(1);
    let child = app.peek_subagent(0, "opencode-tool-task").unwrap();
    assert_eq!(app.selected, Some(1));
    assert_eq!(app.peek_subagent(0, "opencode-tool-task"), Some(child));
    assert_eq!(app.open_subagent(0, "opencode-tool-task"), Some(child));
    assert_ne!(child, 1);
    assert_eq!(app.sessions[child].scope, app.sessions[0].scope);
    assert_eq!(app.selected, Some(child));
    assert_eq!(app.sessions[0].draft, "Parent draft");
    assert_eq!(app.subagent_parent(child), Some(0));
    assert_eq!(app.open_subagent(0, "opencode-tool-task"), Some(child));
    assert_eq!(app.sessions.len(), 3);
    let commands = app.commands();
    let resumes: Vec<_> = commands
        .iter()
        .filter_map(|(_, cmd)| {
            if let Command::Resume(key) = cmd {
                Some(key)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(resumes.len(), 1);
    assert_eq!(resumes[0].id, "ses_child");
    assert_eq!(resumes[0].scope.project, "/project");
    app.sessions[child].archive_fixture(ArchiveFixture::Forgotten);
    assert!(app.open_subagent(0, "opencode-tool-task").is_none());
    assert!(app.open_subagent(0, "missing").is_none());
}
