use super::*;
use crate::application::{ArchiveFixture, Provider, Scope};
use caelum_protocol::{GitRepositoryStatus, ProjectInfo, SessionDetails};
use serde_json::json;

const URL: &str = "https://github.com/example/repository/pull/42";

fn workspace() -> Workspace {
    let mut app = Workspace::default();
    for path in ["/repository", "/worktrees/feature", "/other"] {
        app.projects.push(ProjectInfo {
            path: path.into(),
            name: path.into(),
        });
    }
    app.project_roots
        .insert("/worktrees/feature".into(), "/repository".into());
    app.services
        .reviews
        .entry(("/repository".into(), URL.into()))
        .or_default()
        .set_details(
            serde_json::from_value(json!({
                "number": 42, "title": "Feature", "url": URL, "isDraft": false,
                "body": "", "state": "OPEN", "author": null, "baseRefName": "main",
                "headRefName": "feature", "headRefOid": "a".repeat(40),
                "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
                "additions": 1, "deletions": 0, "changedFiles": 1,
                "reviewDecision": null, "statusCheckRollup": []
            }))
            .unwrap(),
        );
    app
}

fn session(app: &mut Workspace, project: &str, provider: Provider, branch: &str) -> usize {
    let index = app.new_session(Scope {
        project: project.into(),
        provider,
    });
    app.sessions[index].remote_id = Some("example-session".into());
    app.sessions[index].details = Some(SessionDetails {
        branch: Some(branch.into()),
        branches: vec![],
        branch_is_current: false,
        changes: None,
    });
    index
}

#[test]
fn review_session_follows_recorded_history_across_worktrees_without_crossing_repositories() {
    let mut app = workspace();
    let owner = session(
        &mut app,
        "/worktrees/feature",
        Provider::OpenCode,
        "follow-up",
    );
    app.sessions[owner]
        .details
        .as_mut()
        .unwrap()
        .branches
        .push("feature".into());
    session(&mut app, "/repository", Provider::Codex, "main");
    session(&mut app, "/other", Provider::OpenCode, "feature");
    assert_eq!(app.pull_request_session("/repository", URL), Some(owner));

    app.new_session(Scope {
        project: "/repository".into(),
        provider: Provider::Codex,
    });
    app.services
        .checkouts
        .entry("/repository".into())
        .or_default()
        .git = Some(GitRepositoryStatus {
        project_path: "/repository".into(),
        repository_root: Some("/repository".into()),
        branch: Some("feature".into()),
        changes: vec![],
    });
    assert_eq!(app.pull_request_session("/repository", URL), Some(owner));
    app.sessions[owner].details = None;
    assert_eq!(app.pull_request_session("/repository", URL), None);
}

#[test]
fn shared_branch_prefers_active_sessions_then_selection_and_recent_activity() {
    let mut app = workspace();
    let older = session(&mut app, "/repository", Provider::Codex, "feature");
    let newer = session(
        &mut app,
        "/worktrees/feature",
        Provider::OpenCode,
        "feature",
    );
    let other = session(&mut app, "/other", Provider::Codex, "feature");
    app.sessions[older].last_activity = Some(10);
    app.sessions[newer].last_activity = Some(20);
    assert_eq!(app.pull_request_session("/repository", URL), Some(newer));
    app.select(older);
    assert_eq!(app.pull_request_session("/repository", URL), Some(older));

    app.sessions[older].archive_fixture(ArchiveFixture::Stable(true));
    assert_eq!(app.pull_request_session("/repository", URL), Some(newer));
    app.sessions[newer].archive_fixture(ArchiveFixture::Stable(true));
    app.select(other);
    assert_eq!(app.pull_request_session("/repository", URL), Some(newer));
    assert!(app.sessions[newer].archived());
    assert_eq!(app.selected, Some(other));
}

#[test]
fn unavailable_sessions_and_missing_review_metadata_do_not_redirect_navigation() {
    for unavailable in [
        "forgotten",
        "deleting",
        "archiving",
        "removed",
        "loading",
        "empty-head",
    ] {
        let mut app = workspace();
        let owner = session(&mut app, "/worktrees/feature", Provider::Codex, "feature");
        let other = session(&mut app, "/other", Provider::OpenCode, "feature");
        match unavailable {
            "forgotten" => app.sessions[owner].archive_fixture(ArchiveFixture::Forgotten),
            "deleting" => app.sessions[owner].archive_fixture(ArchiveFixture::Deleting),
            "archiving" => app.sessions[owner].archive_fixture(ArchiveFixture::Archiving),
            "removed" => app.projects.retain(|p| p.path != "/worktrees/feature"),
            _ => {
                let review = app
                    .services
                    .reviews
                    .get_mut(&("/repository".into(), URL.into()))
                    .unwrap();
                if unavailable == "loading" {
                    review.clear_details();
                } else {
                    review.edit_details(|details| details.head_ref_name.clear());
                }
            }
        }
        assert_eq!(
            app.pull_request_session("/repository", URL),
            None,
            "{unavailable}"
        );
        assert_eq!(app.selected, Some(other));
    }
}

#[test]
fn session_pull_request_stays_strictly_attributed_to_recorded_session_branches() {
    let mut app = workspace();
    let active = session(&mut app, "/repository", Provider::Codex, "feature");
    let older = session(&mut app, "/repository", Provider::Codex, "old-branch");
    let unrecorded = app.new_session(Scope {
        project: "/repository".into(),
        provider: Provider::Codex,
    });
    let checkout = app
        .services
        .checkouts
        .entry("/repository".into())
        .or_default();
    checkout.git = Some(GitRepositoryStatus {
        project_path: "/repository".into(),
        repository_root: Some("/repository".into()),
        branch: Some("feature".into()),
        changes: vec![],
    });
    checkout
        .pull_requests
        .push(caelum_protocol::GitHubPullRequest {
            number: 42,
            title: "Feature".into(),
            url: URL.into(),
            is_draft: false,
            base_ref_name: "main".into(),
            review_decision: None,
        });
    app.services
        .record_discovered_pull_request("/repository", "feature", URL);

    assert_eq!(
        app.session_pull_request(&app.sessions[active])
            .map(|pr| pr.number),
        Some(42)
    );
    assert_eq!(app.session_pull_request(&app.sessions[older]), None);
    assert_eq!(app.session_pull_request(&app.sessions[unrecorded]), None);
}

#[test]
fn completing_a_turn_rediscovers_pull_requests_on_recorded_branches() {
    use crate::application::{Command, Event, Reply, Task, TaskReply};

    let mut app = workspace();
    let index = session(&mut app, "/repository", Provider::Codex, "feature");
    app.sessions[index].remote_id = Some("thread-1".into());
    app.sessions[index].conversation.set_working(true);
    let _ = app.commands();

    app.apply(Event::Codex(caelum_protocol::CodexEvent {
        project_path: "/repository".into(),
        method: "turn/completed".into(),
        params: serde_json::json!({
            "threadId": "thread-1",
            "turn": { "id": "turn-1", "status": "completed" }
        }),
        request_id: None,
    }));

    let discover = app
        .commands()
        .into_iter()
        .find_map(|(id, cmd)| match cmd {
            Command::Task(Task::Discover { project, branch })
                if project == "/repository" && branch == "feature" =>
            {
                Some(id)
            }
            _ => None,
        })
        .expect("turn completion should re-discover PRs for the session's branch");

    let pr = caelum_protocol::GitHubPullRequest {
        number: 42,
        title: "Feature".into(),
        url: URL.into(),
        is_draft: false,
        base_ref_name: "main".into(),
        review_decision: None,
    };
    app.apply(Event::Completed(
        discover,
        Ok(Reply::Task(TaskReply::Discovered(Some(pr)))),
    ));

    assert_eq!(
        app.session_pull_request(&app.sessions[index])
            .map(|pr| pr.number),
        Some(42)
    );
}
