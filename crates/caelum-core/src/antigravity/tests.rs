use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

use caelum_protocol::ApprovalDecision;

use super::AntigravityProvider;
use crate::{approvals::ApprovalBroker, processes::ProcessRegistry};

const FAKE_SERVER: &str = include_str!("tests/fixtures/server.py");

struct Fixture {
    root: PathBuf,
    project: PathBuf,
    server: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "caelum-antigravity-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let project = root.join("project");
        let server = root.join("fake-agy");
        fs::create_dir_all(&project).unwrap();
        crate::test_support::write_executable(&server, FAKE_SERVER);
        Self {
            root,
            project,
            server,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn starts_lists_streams_replays_history_and_resolves_permissions() {
    let fixture = Fixture::new();
    let processes = Arc::new(ProcessRegistry::default());
    let approvals = ApprovalBroker::default();
    let provider = AntigravityProvider::with_executable(
        Some(fixture.server.clone()),
        processes,
        approvals.clone(),
    );
    let events = provider.subscribe();

    let choices = provider.choices(&fixture.project).unwrap();
    assert_eq!(choices.default_model.as_deref(), Some("gemini-4-neon"));
    assert_eq!(choices.models.len(), 2);
    assert_eq!(choices.models[0].efforts.len(), 3);
    assert_eq!(choices.agents.len(), 2);

    let started = provider
        .start_session_with_options(
            &fixture.project,
            Some("claude-opus-5-5"),
            Some("medium"),
            Some("plan"),
        )
        .unwrap();
    let session_id = started.session.id.clone();

    let listed = provider.list_sessions(&fixture.project).unwrap();
    assert_eq!(listed.sessions.len(), 1);
    assert_eq!(listed.sessions[0].id, session_id);

    provider
        .send_prompt_with_options(
            &fixture.project,
            &session_id,
            "hello world",
            None,
            None,
            None,
        )
        .unwrap();

    let mut saw_message = String::new();
    let mut saw_completed = false;
    for _ in 0..50 {
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        if event.method == "agent_message_chunk" {
            saw_message.push_str(event.params["content"]["text"].as_str().unwrap_or_default());
        }
        if event.method == "antigravity/turnCompleted" {
            saw_completed = true;
            break;
        }
    }
    assert!(saw_completed);
    assert_eq!(saw_message, "echo:claude-opus-5-5:medium:plan:hello world");

    let (_, history) = provider
        .resume_session(&fixture.project, &session_id)
        .unwrap();
    assert!(history.data.iter().any(|item| item["type"] == "user"));
    assert!(history.data.iter().any(|item| item["type"] == "reasoning"));
    assert!(history.data.iter().any(|item| item["type"] == "tool"));
    assert!(history.data.iter().any(|item| item["type"] == "assistant"));

    let mut conv = crate::application::Conversation::default();
    conv.history(
        crate::application::Provider::Antigravity,
        &history.data,
        false,
    );
    let read_entry = conv
        .entries
        .iter()
        .find(|e| e.id == "acp-tool-call-read")
        .expect("read tool entry");
    let read_tool = crate::application::presentation::Tool::from_entry(read_entry, 0);
    assert_eq!(read_tool.name, "Read");
    assert_eq!(read_tool.paths, ["README.md"]);
    assert_eq!(read_tool.output, "# Caelum\nHello");
    assert!(read_tool.details.contains(&"From 1".to_owned()));
    assert!(read_tool.details.contains(&"Limit 5".to_owned()));

    let edit_entry = conv
        .entries
        .iter()
        .find(|e| e.id == "acp-tool-call-edit")
        .expect("edit tool entry");
    let edit_tool = crate::application::presentation::Tool::from_entry(edit_entry, 0);
    assert_eq!(edit_tool.name, "Patch");
    assert_eq!(edit_tool.paths, ["src/lib.rs"]);
    assert!(edit_tool.patch.contains("-fn old() {}"));
    assert!(edit_tool.patch.contains("+fn new() {"));

    let write_entry = conv
        .entries
        .iter()
        .find(|e| e.id == "acp-tool-call-write")
        .expect("write tool entry");
    let write_tool = crate::application::presentation::Tool::from_entry(write_entry, 0);
    assert_eq!(write_tool.name, "Patch");
    assert_eq!(write_tool.paths, ["notes.md"]);
    assert!(write_tool.patch.contains("+line 1"));

    let run_entry = conv
        .entries
        .iter()
        .find(|e| e.id == "acp-tool-call-run")
        .expect("run tool entry");
    let run_tool = crate::application::presentation::Tool::from_entry(run_entry, 0);
    assert_eq!(run_tool.name, "Run");
    assert_eq!(run_tool.summary, "cargo test");
    assert_eq!(run_tool.output, "test result: ok.");

    let subagent_entry = conv
        .entries
        .iter()
        .find(|e| e.id == "acp-tool-call-subagent")
        .expect("subagent tool entry");
    let subagent = crate::application::subagents::Subagent::from_entry(subagent_entry)
        .expect("subagent presentation");
    assert_eq!(subagent.agent, "research");
    assert_eq!(subagent.description, "Codebase Researcher");
    assert_eq!(subagent.prompt, "Inspect lib.rs");

    let details = provider
        .session_details(&fixture.project, &session_id)
        .unwrap();
    let changes = details.changes.expect("session diff line changes");
    assert_eq!(changes.additions, 5);
    assert_eq!(changes.deletions, 1);

    let approval_updates = approvals.subscribe();
    provider
        .send_prompt_with_options(
            &fixture.project,
            &session_id,
            "need-permission",
            None,
            None,
            None,
        )
        .unwrap();

    let prompt = loop {
        let pending = approval_updates
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        if let Some(first) = pending.into_iter().next() {
            break first;
        }
    };
    assert_eq!(prompt.provider, "antigravity");
    approvals
        .resolve(
            &prompt.provider,
            &prompt.project_path,
            &prompt.request_id,
            ApprovalDecision::AllowOnce,
        )
        .unwrap();

    let mut perm_reply = String::new();
    for _ in 0..20 {
        let event = events.recv_timeout(Duration::from_secs(5)).unwrap();
        if event.method == "agent_message_chunk" {
            perm_reply.push_str(event.params["content"]["text"].as_str().unwrap_or_default());
        }
        if event.method == "antigravity/turnCompleted" {
            break;
        }
    }
    assert_eq!(perm_reply, "permission:allow_once");

    provider.shutdown();
}

#[test]
fn archives_restores_renames_and_deletes_sessions_with_persistence() {
    let fixture = Fixture::new();
    let metadata_path = fixture.root.join("antigravity-sessions.json");
    let processes = Arc::new(ProcessRegistry::default());
    let approvals = ApprovalBroker::default();
    let provider = AntigravityProvider::with_executable(
        Some(fixture.server.clone()),
        processes.clone(),
        approvals.clone(),
    );
    provider.open_metadata(metadata_path.clone()).unwrap();

    let started = provider
        .start_session_with_options(&fixture.project, None, None, None)
        .unwrap();
    let session_id = started.session.id;

    assert_eq!(
        provider
            .list_sessions(&fixture.project)
            .unwrap()
            .sessions
            .len(),
        1
    );
    assert!(provider
        .list_archived_sessions(&fixture.project)
        .unwrap()
        .sessions
        .is_empty());

    assert!(provider
        .delete_session(&fixture.project, &session_id)
        .is_err());

    provider
        .rename(&fixture.project, &session_id, "Custom ACP Title")
        .unwrap();
    assert_eq!(
        provider.list_sessions(&fixture.project).unwrap().sessions[0].title,
        "Custom ACP Title"
    );

    provider
        .set_session_archived(&fixture.project, &session_id, true)
        .unwrap();
    assert!(provider
        .list_sessions(&fixture.project)
        .unwrap()
        .sessions
        .is_empty());
    let archived = provider.list_archived_sessions(&fixture.project).unwrap();
    assert_eq!(archived.sessions.len(), 1);
    assert_eq!(archived.sessions[0].id, session_id);
    assert_eq!(archived.sessions[0].title, "Custom ACP Title");
    provider.shutdown();

    let reopened =
        AntigravityProvider::with_executable(Some(fixture.server.clone()), processes, approvals);
    reopened.open_metadata(metadata_path).unwrap();
    let (resumed, _) = reopened
        .resume_session(&fixture.project, &session_id)
        .unwrap();
    assert_eq!(resumed.session.title, "Custom ACP Title");

    reopened
        .set_session_archived(&fixture.project, &session_id, false)
        .unwrap();
    assert_eq!(
        reopened
            .list_sessions(&fixture.project)
            .unwrap()
            .sessions
            .len(),
        1
    );
    assert!(reopened
        .list_archived_sessions(&fixture.project)
        .unwrap()
        .sessions
        .is_empty());

    reopened
        .set_session_archived(&fixture.project, &session_id, true)
        .unwrap();
    reopened
        .delete_session(&fixture.project, &session_id)
        .unwrap();
    assert!(reopened
        .list_sessions(&fixture.project)
        .unwrap()
        .sessions
        .is_empty());
    assert!(reopened
        .list_archived_sessions(&fixture.project)
        .unwrap()
        .sessions
        .is_empty());
    assert!(reopened
        .resume_session(&fixture.project, &session_id)
        .is_err());

    reopened.shutdown();
}
