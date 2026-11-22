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
    for _ in 0..20 {
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
