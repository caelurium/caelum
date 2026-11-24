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
fn fix_failing_check_reuses_idle_checkout_and_preserves_parent_draft() {
    let mut app = workspace();
    let parent = session(
        &mut app,
        "/worktrees/feature",
        Provider::OpenCode,
        "feature",
    );
    app.sessions[parent].draft = "Keep my parent draft".into();
    let _ = app.commands();

    let details_url = "https://github.com/example/repository/actions/runs/999/job/555";
    let child = app
        .fix_failing_check(
            "/repository".into(),
            URL.into(),
            "cargo test".into(),
            Some(details_url.into()),
        )
        .expect("idle checkout spawns child immediately");

    assert_ne!(child, parent);
    assert_eq!(app.selected, Some(child));
    assert_eq!(app.sessions[child].scope.project, "/worktrees/feature");
    assert_eq!(app.sessions[parent].draft, "Keep my parent draft");
    assert_eq!(app.origin_parent_session(child), Some(parent));
    assert!(matches!(
        &app.sessions[child].origin,
        Some(SessionOrigin::PullRequestCheck {
            pr_number: 42,
            run_id: Some(999),
            check_name,
            ..
        }) if check_name == "cargo test"
    ));

    let commands = app.commands();
    let (log_req, _) = commands
        .iter()
        .find(|(_, c)| {
            matches!(
                c,
                crate::application::Command::Task(Task::CheckLogs { check_name, .. })
                    if check_name == "cargo test"
            )
        })
        .expect("dispatches CheckLogs task");
    assert!(!commands.iter().any(|(_, c)| matches!(
        c,
        crate::application::Command::Task(Task::CreateWorktree { .. })
    )));

    app.apply(crate::application::Event::Completed(
        *log_req,
        Ok(crate::application::Reply::Task(
            crate::application::TaskReply::CheckLogs(GitHubCheckLogs {
                run_id: 999,
                job_id: Some(555),
                check_name: "cargo test".into(),
                excerpt: "test failed: assertion".into(),
                truncated: false,
                url: details_url.into(),
            }),
        )),
    ));
    assert_eq!(app.sessions[child].attachments.len(), 1);
    assert_eq!(
        app.sessions[child].attachments[0].content,
        "test failed: assertion"
    );

    // Calling again for the same check focuses the existing child instead of duplicating.
    let again = app.fix_failing_check(
        "/repository".into(),
        URL.into(),
        "cargo test".into(),
        Some(details_url.into()),
    );
    assert_eq!(again, Some(child));
}

#[test]
fn fix_failing_check_creates_isolated_worktree_only_when_turn_is_running() {
    let mut app = workspace();
    let parent = session(&mut app, "/worktrees/feature", Provider::Codex, "feature");
    app.sessions[parent].conversation.set_working(true);
    let _ = app.commands();

    let immediate = app.fix_failing_check("/repository".into(), URL.into(), "Clippy".into(), None);
    assert_eq!(immediate, None);
    assert!(app.ci_fix_pending(URL, "Clippy"));

    let commands = app.commands();
    let (wt_req, _) = commands
        .iter()
        .find(|(_, c)| {
            matches!(
                c,
                crate::application::Command::Task(Task::CreateWorktree { base, .. })
                    if base == "feature"
            )
        })
        .expect("busy checkout triggers isolated CreateWorktree");

    app.apply(crate::application::Event::Completed(
        *wt_req,
        Ok(crate::application::Reply::Task(
            crate::application::TaskReply::Worktree(caelum_protocol::GitWorktree {
                path: "/worktrees/ci-fix-pr-42-clippy".into(),
                branch: Some("ci-fix-pr-42-clippy".into()),
                locked: None,
                prunable: None,
                main: false,
                available: true,
                label: Some("ci-fix-pr-42-clippy".into()),
                project: ProjectInfo {
                    name: "ci-fix-pr-42-clippy".into(),
                    path: "/worktrees/ci-fix-pr-42-clippy".into(),
                },
            }),
        )),
    ));
    assert!(!app.ci_fix_pending(URL, "Clippy"));
    let child = app
        .ci_fix_child_session(URL, &"a".repeat(40), "Clippy")
        .expect("child created in isolated worktree");
    assert_eq!(
        app.sessions[child].scope.project,
        "/worktrees/ci-fix-pr-42-clippy"
    );
    assert_eq!(app.origin_parent_session(child), Some(parent));
}
