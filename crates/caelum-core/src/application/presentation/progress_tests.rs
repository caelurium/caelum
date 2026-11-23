use super::TurnPreview;
use crate::application::{Entry, EntryKind, Provider, Scope, Workspace};
use crate::remote::{Line, Run, Speaker, Thread, ToolState, Turn, TurnEnd};
use serde_json::json;

#[test]
fn a_local_card_uses_only_the_active_turn_and_never_tool_output_or_attached_context() {
    let mut app = Workspace::default();
    let i = app.new_session(Scope {
        project: "/project".into(),
        provider: Provider::Codex,
    });
    let text = |app: &Workspace| app.turn_preview(&app.sessions[i]).text;
    let c = &mut app.sessions[i].conversation;
    c.apply(Provider::Codex, "item/completed", &json!({"_created":1000,"item":{"id":"old","type":"agentMessage","text":"Previous turn only"}}));
    c.begin_turn(None, 1000);
    c.optimistic(
        "prompt".into(),
        "Check the retry path.\n\n<caelum-file-context>\ncaptured file\n</caelum-file-context>"
            .into(),
    );
    assert_eq!(text(&app).as_deref(), Some("Check the retry path."));

    let c = &mut app.sessions[i].conversation;
    c.apply(
        Provider::Codex,
        "item/reasoning/textDelta",
        &json!({"itemId":"thought","delta":"raw reasoning"}),
    );
    c.apply(Provider::Codex, "item/agentMessage/delta", &json!({"itemId":"reply","delta":"<system-reminder>hidden</system-reminder>Tracing the retry."}));
    assert_eq!(text(&app).as_deref(), Some("Tracing the retry."));
    app.sessions[i].conversation.apply(Provider::Codex, "item/started", &json!({"_created":1500,"item":{
        "id":"cmd","type":"commandExecution","command":"cargo test --private-argument", "aggregatedOutput":"raw output"
    }}));
    assert_eq!(text(&app).as_deref(), Some("Running cargo"));

    let c = &mut app.sessions[i].conversation;
    c.apply(Provider::Codex, "turn/completed", &json!({"_created":2000}));
    c.apply(Provider::Codex, "turn/started", &json!({"_created":2000}));
    assert_eq!(text(&app), None);
    assert_eq!(app.turn_preview(&app.sessions[i]).started_at, Some(2000));
}

#[test]
fn a_running_child_stays_in_the_preview_when_its_parent_turn_settles() {
    let mut app = Workspace::default();
    let i = app.new_session(Scope {
        project: "/project".into(),
        provider: Provider::OpenCode,
    });
    let c = &mut app.sessions[i].conversation;
    c.apply(Provider::OpenCode, "session.tool.called", &json!({"id":"task","name":"subagent",
        "input":{"agent":"explore","description":"Trace the reconnect timer","prompt":"Detailed child instructions"},
        "metadata":{"sessionID":"ses_child","status":"running"},"_created":1200}));
    c.settle();
    c.entries.push(Entry::new(
        "later".into(),
        EntryKind::Assistant,
        "Previous reply".into(),
        true,
    ));
    let preview = app.turn_preview(&app.sessions[i]);
    assert_eq!(preview.text.as_deref(), Some("Trace the reconnect timer"));
    assert_eq!(preview.started_at, Some(1200));
}

#[test]
fn a_managed_card_follows_the_prompt_boundary_through_queue_tool_and_reply() {
    let run = Run {
        provider: crate::remote::VmProvider::Codex,
        conversation: "chat".into(),
        prompt: "prompt-id".into(),
        id: "run".into(),
    };
    let mut thread = Thread {
        turn: Turn::Queued(run.clone()),
        lines: vec![
            Line::new(Speaker::You, "Earlier prompt"),
            Line::new(Speaker::Codex, "Earlier reply"),
            Line::new(Speaker::Outcome(TurnEnd::Finished), "Done"),
        ],
        ..Default::default()
    };
    let preview = |t: &Thread| TurnPreview::managed(t).unwrap().text;
    assert_eq!(preview(&thread), None);
    thread
        .lines
        .push(Line::new(Speaker::You, "Check the timer."));
    assert_eq!(preview(&thread).as_deref(), Some("Check the timer."));
    thread.turn = Turn::Following(run);
    thread
        .lines
        .push(Line::new(Speaker::ReasoningSummary, "Reasoning"));
    thread.lines.push(Line {
        tool_detail: Some("src/timer.rs".into()),
        ..Line::new(Speaker::Tool(ToolState::Running), "Read")
    });
    assert_eq!(preview(&thread).as_deref(), Some("Reading src/timer.rs"));
    thread.lines.push(Line::new(
        Speaker::Codex,
        "Found the timer. ![diagram](data:image/png;base64,unfinished",
    ));
    assert_eq!(preview(&thread).as_deref(), Some("Found the timer."));
}
