use crate::{Workspace, live::Live};
use caelum_core::application::{Provider, Scope, SessionScope};
use serde_json::json;

const NOW: u64 = 1_800_000_000_000;

pub(super) fn scene(state: &mut Workspace, name: &str) -> bool {
    if !matches!(
        name,
        "session-feed"
            | "conversation-overview"
            | "session-feed-current"
            | "session-feed-empty"
            | "session-feed-many"
            | "session-feed-colours"
    ) {
        return false;
    }
    let l = state.live.as_mut().unwrap();
    l.feed.set_display_clock(NOW);
    for (i, session) in l.app.sessions.iter_mut().enumerate() {
        session.last_activity = Some(NOW - (i as u64 + 1) * 900_000);
    }
    let mut projects = vec![
        (
            "/projects/rune",
            "rune",
            "Keep keyboard focus in the editor",
            "fix/editor-focus",
            Provider::Codex,
            2,
        ),
        (
            "/projects/website",
            "website",
            "Ship the refreshed landing page",
            "feat/homepage",
            Provider::OpenCode,
            7,
        ),
        (
            "/projects/tools",
            "developer-tools",
            "Make local builds incremental",
            "build/dev-app",
            Provider::Codex,
            45,
        ),
    ];
    if name == "session-feed-colours" {
        projects.extend([
            (
                "/projects/glance",
                "glance",
                "Polish the dashboard",
                "feat/dashboard",
                Provider::Codex,
                1,
            ),
            (
                "/projects/singularity",
                "singularity",
                "Keep workspace state consistent",
                "fix/state",
                Provider::OpenCode,
                3,
            ),
        ]);
    }
    for (path, name, title, branch, provider, age) in projects {
        l.app.projects.push(caelum_protocol::ProjectInfo {
            name: name.into(),
            path: path.into(),
        });
        let i = l.app.new_session(Scope {
            project: path.into(),
            provider,
        });
        let session = &mut l.app.sessions[i];
        session.title = title.into();
        session.remote_id = Some(format!("fixture-{i}"));
        session.last_activity = Some(NOW - age * 60_000);
        session.details = Some(caelum_protocol::SessionDetails {
            branch: Some(branch.into()),
            branches: vec![branch.into()],
            branch_is_current: true,
            changes: None,
        });
        session.unread = age == 7;
        let checkout = l.app.services.checkouts.entry(path.into()).or_default();
        checkout.git = Some(caelum_protocol::GitRepositoryStatus {
            project_path: path.into(),
            repository_root: Some(path.into()),
            branch: Some(branch.into()),
            changes: vec![],
        });
        checkout
            .pull_requests
            .push(caelum_protocol::GitHubPullRequest {
                number: 42,
                title: title.into(),
                url: format!("https://github.com/example/{name}/pull/42"),
                is_draft: false,
                base_ref_name: "main".into(),
                review_decision: None,
            });
    }
    if name == "session-feed-current" {
        l.app.preferences.session_scope = SessionScope::CurrentProject;
    }
    if name == "session-feed-empty" {
        l.panels.sessions.text = "No matching session".into();
    }
    if name == "session-feed-many" {
        many(l, 2000);
    }
    l.app.select(0);
    l.sync_editors();
    if name == "conversation-overview" {
        overview(l);
        crate::panels::conversations::open(state, &mut rune::Tx::default());
    }
    true
}

fn overview(l: &mut Live) {
    use caelum_core::application::{Entry, EntryKind, QueuedPrompt};
    let session = &mut l.app.sessions[1];
    session.title = "Reconnect after sleep".into();
    session.remote_id = Some("reconnect-example".into());
    l.app.questions.push(caelum_protocol::QuestionPrompt {
        provider: session.scope.provider.as_str().into(),
        project_path: session.scope.project.clone(),
        session_id: "reconnect-example".into(),
        request_id: "retry-choice".into(),
        title: "Questions".into(),
        fields: vec![
            json!({"key":"retry","type":"boolean","title":"Retry policy",
            "description":"Should a manual disconnect stop automatic retries?", "default":true}),
        ],
    });
    l.app.sessions[2].title = "Review the provider adapters".into();
    l.app.sessions[2].conversation.entries.push(Entry::new(
        "adapters".into(),
        EntryKind::Assistant,
        "All three providers now keep their own session and model defaults.".into(),
        true,
    ));
    let focus = &mut l.app.sessions[3].conversation;
    focus.begin_turn(None, NOW - 180_000);
    focus.apply(Provider::Codex, "item/agentMessage/delta",
        &json!({"itemId":"focus-progress", "delta":"I’m tracing where the editor loses focus when the inspector opens."}));
    let session = &l.app.sessions[4];
    l.app.approvals.push(caelum_protocol::ApprovalPrompt {
        provider: session.scope.provider.as_str().into(),
        project_path: session.scope.project.clone(),
        session_id: session.remote_id.clone(),
        request_id: "preview-site".into(),
        summary: "Allow the agent to start the preview server?".into(),
        description: String::new(),
    });
    l.app.sessions[5].queue.push(QueuedPrompt {
        text: "Compare the cold and incremental build times before the next release.".into(),
        options: Default::default(),
    });
}

pub(super) fn sidebar_projects(l: &mut Live, project: &str) {
    for (path, name) in [
        ("/projects/rune", "rune"),
        ("/projects/website", "website"),
        ("/projects/tools", "developer-tools"),
    ] {
        l.app.projects.push(caelum_protocol::ProjectInfo {
            path: path.into(),
            name: name.into(),
        });
        let index = l.app.new_session(Scope {
            project: path.into(),
            provider: Provider::Codex,
        });
        l.app.sessions[index].draft = format!("Independent draft for {name}");
        l.app.services.checkouts.entry(path.into()).or_default().git = Some(
            serde_json::from_value(json!({
                "projectPath":path,"repositoryRoot":path,"branch":"main","changes":[]
            }))
            .unwrap(),
        );
    }
    for n in 0..6 {
        let index = l.app.new_session(Scope {
            project: project.into(),
            provider: Provider::Codex,
        });
        l.app.sessions[index].title = format!("Workspace task {}", n + 1);
    }
    l.app
        .services
        .checkouts
        .get_mut(project)
        .unwrap()
        .pull_requests = [
        "Give projects and reviews room to breathe",
        "Restore independent session drafts",
        "Improve native keyboard navigation",
        "Polish the compact review workspace",
        "Keep provider requests scoped",
        "Refresh the desktop documentation",
    ]
    .iter()
    .enumerate()
    .map(|(i, title)| caelum_protocol::GitHubPullRequest {
        number: 101 + i as u64,
        title: (*title).into(),
        url: format!("https://github.com/caelurium/caelum/pull/{}", 101 + i),
        is_draft: false,
        base_ref_name: "main".into(),
        review_decision: None,
    })
    .collect();
    l.app.select(0);
}

fn many(l: &mut Live, count: usize) {
    for n in 0..count {
        let i = l.app.new_session(Scope {
            project: "/projects/rune".into(),
            provider: Provider::Codex,
        });
        l.app.sessions[i].title = format!("Historical session {n}");
        l.app.sessions[i].last_activity = Some(NOW - (n as u64 + 1) * 86_400_000);
    }
}
