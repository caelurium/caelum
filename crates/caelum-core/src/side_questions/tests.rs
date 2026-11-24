use super::*;
use crate::application::{Conversation, Entry, EntryKind, Scope};
use crate::processes::ProcessRegistry;
use std::sync::{mpsc, Arc};

#[cfg(unix)]
fn python_script(code: &str) -> Command {
    let mut cmd = Command::new("python3");
    cmd.arg("-u").arg("-c").arg(code);
    cmd
}

#[test]
fn prompt_bounds_history_strips_attachments_and_rejects_oversized_question() {
    let mut history = Conversation::default();
    for i in 0..15 {
        history.entries.push(Entry::new(
            format!("u-{i}"),
            EntryKind::User,
            format!("user-{i}\n\n<caelum-attachment>secret</caelum-attachment>"),
            true,
        ));
        history.entries.push(Entry::new(
            format!("a-{i}"),
            EntryKind::Assistant,
            format!("assistant-{i}"),
            true,
        ));
    }
    let rendered = prompt(&history, "What changed?").unwrap();
    assert!(rendered.contains("Side question:\nWhat changed?"));
    assert!(!rendered.contains("caelum-attachment"));
    assert!(!rendered.contains("secret"));
    assert!(rendered.contains("assistant-14"));
    assert!(!rendered.contains("user-0"));

    let huge = "x".repeat(128 * 1024 + 1);
    assert!(prompt(&history, &huge).is_err());
}

#[cfg(unix)]
#[test]
fn codex_side_turn_runs_read_only_ephemeral_and_streams_answer() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap().to_owned();
    let script = r#"
import json, sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "thread/start":
        p = msg["params"]
        assert p["ephemeral"] is True
        assert p["sandbox"] == "read-only"
        assert p["approvalPolicy"] == "never"
        assert p["config"]["mcp_servers"] == {}
        assert p["config"]["web_search"] == "disabled"
        print(json.dumps({"id": mid, "result": {"thread": {"id": "side-thread", "cwd": p["cwd"]}}}), flush=True)
    elif method == "turn/start":
        # Send an early delta before the turn/start reply to verify buffering.
        print(json.dumps({"method": "item/agentMessage/delta", "params": {"threadId": "side-thread", "turnId": "t-1", "delta": "Hello "}}), flush=True)
        # Send a server-initiated request that must be rejected cleanly.
        print(json.dumps({"id": 99, "method": "tool/exec", "params": {}}), flush=True)
        print(json.dumps({"id": mid, "result": {"turn": {"id": "t-1"}}}), flush=True)
        print(json.dumps({"method": "item/agentMessage/delta", "params": {"threadId": "side-thread", "turnId": "t-1", "delta": "world"}}), flush=True)
        print(json.dumps({"method": "item/completed", "params": {"threadId": "side-thread", "turnId": "t-1", "item": {"type": "agentMessage", "text": "Hello world"}}}), flush=True)
        print(json.dumps({"method": "turn/completed", "params": {"threadId": "side-thread", "turnId": "t-1", "turn": {"id": "t-1", "status": "completed"}}}), flush=True)
        break
"#;
    let (_tx_cancel, until) = watch::channel(());
    let turn = Turn {
        generation: 7,
        text: "Explain this".into(),
        model: Some("gpt-5.3-codex".into()),
        until,
    };
    let key = SessionKey {
        scope: Scope {
            provider: Provider::Codex,
            project: root,
        },
        id: "parent-thread".into(),
    };
    let (event_tx, event_rx) = mpsc::sync_channel(16);
    let output = Output::default();
    output.connect(event_tx, Arc::new(|| {}));
    let registry = ProcessRegistry::default();
    let answer = execute(
        &registry,
        python_script(script),
        dir.path(),
        &key,
        &turn,
        &output,
    )
    .unwrap();
    assert_eq!(answer, "Hello world");
    let events: Vec<_> = event_rx.try_iter().collect();
    assert!(events.iter().any(
        |e| matches!(e, Event::SideAnswer(k, 7, text) if k == &key && text.contains("Hello"))
    ));
}

#[cfg(unix)]
#[test]
fn codex_side_turn_rejects_reusing_parent_thread() {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"
import json, sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "thread/start":
        p = msg["params"]
        print(json.dumps({"id": mid, "result": {"thread": {"id": "parent-thread", "cwd": p["cwd"]}}}), flush=True)
        break
"#;
    let (_tx_cancel, until) = watch::channel(());
    let turn = Turn {
        generation: 1,
        text: "Question".into(),
        model: None,
        until,
    };
    let key = SessionKey {
        scope: Scope {
            provider: Provider::Codex,
            project: dir.path().to_str().unwrap().into(),
        },
        id: "parent-thread".into(),
    };
    let err = execute(
        &ProcessRegistry::default(),
        python_script(script),
        dir.path(),
        &key,
        &turn,
        &Output::default(),
    )
    .unwrap_err();
    assert!(err.contains("unexpected side thread"));
}

#[cfg(unix)]
#[test]
fn antigravity_side_turn_enforces_plan_mode_and_cancels_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"
import json, sys

saw_cancel = False
for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if mid == 50 and "result" in msg:
        saw_cancel = msg["result"]["outcome"]["outcome"] == "cancelled"
        continue
    if method == "initialize":
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "session/new":
        assert msg["params"]["mcpServers"] == []
        print(json.dumps({"id": mid, "result": {
            "sessionId": "acp-side",
            "modes": {"currentModeId": "code", "availableModes": [{"id": "plan"}, {"id": "code"}]},
            "configOptions": [{"id": "model", "options": [{"value": "gemini-pro"}]}]
        }}), flush=True)
    elif method == "session/set_config_option":
        assert msg["params"]["value"] == "gemini-pro"
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "session/set_mode":
        assert msg["params"]["modeId"] == "plan"
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "session/prompt":
        print(json.dumps({"id": 50, "method": "session/request_permission", "params": {}}), flush=True)
        print(json.dumps({"method": "session/update", "params": {
            "sessionId": "acp-side",
            "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "Plan answer"}}
        }}), flush=True)
        print(json.dumps({"id": mid, "result": {"stopReason": "end_turn"}}), flush=True)
    elif method == "session/close":
        print(json.dumps({"id": mid, "result": {}}), flush=True)
        assert saw_cancel
        break
"#;
    let (_tx_cancel, until) = watch::channel(());
    let turn = Turn {
        generation: 3,
        text: "Summarize".into(),
        model: Some("gemini-pro".into()),
        until,
    };
    let key = SessionKey {
        scope: Scope {
            provider: Provider::Antigravity,
            project: dir.path().to_str().unwrap().into(),
        },
        id: "parent-acp".into(),
    };
    let answer = execute(
        &ProcessRegistry::default(),
        python_script(script),
        dir.path(),
        &key,
        &turn,
        &Output::default(),
    )
    .unwrap();
    assert_eq!(answer, "Plan answer");
}

#[cfg(unix)]
#[test]
fn antigravity_side_turn_fails_if_plan_mode_missing_or_left() {
    let dir = tempfile::tempdir().unwrap();
    let script = r#"
import json, sys

for line in sys.stdin:
    msg = json.loads(line)
    method = msg.get("method")
    mid = msg.get("id")
    if method == "initialize":
        print(json.dumps({"id": mid, "result": {}}), flush=True)
    elif method == "session/new":
        print(json.dumps({"id": mid, "result": {
            "sessionId": "acp-side",
            "modes": {"currentModeId": "code", "availableModes": [{"id": "code"}]}
        }}), flush=True)
        break
"#;
    let (_tx_cancel, until) = watch::channel(());
    let turn = Turn {
        generation: 1,
        text: "Check".into(),
        model: None,
        until,
    };
    let key = SessionKey {
        scope: Scope {
            provider: Provider::Antigravity,
            project: dir.path().to_str().unwrap().into(),
        },
        id: "parent-acp".into(),
    };
    let err = execute(
        &ProcessRegistry::default(),
        python_script(script),
        dir.path(),
        &key,
        &turn,
        &Output::default(),
    )
    .unwrap_err();
    assert!(err.contains("plan mode"));
}

#[test]
fn append_enforces_size_limit_and_answered_rejects_blank() {
    let mut answer = String::new();
    append(&mut answer, "ok").unwrap();
    assert!(append(&mut answer, &"a".repeat(LIMIT)).is_err());
    assert!(answered("   \n".into()).is_err());
    assert_eq!(answered("hi".into()).unwrap(), "hi");
}
