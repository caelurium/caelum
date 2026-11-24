use super::*;
use caelum_protocol::{GitHubPullRequest, GitHubPullRequestDetails, SessionDetails};

fn requested() -> GitHubPullRequest {
    serde_json::from_value(json!({
        "number": 42, "title": "Review session navigation", "url": "https://github.com/example/repo/pull/42",
        "isDraft": false, "reviewDecision": "CHANGES_REQUESTED"
    })).unwrap()
}

fn session(app: &mut Workspace, project: &str, branch: &str) -> usize {
    let index = app.new_session(scope(Provider::Codex, project));
    app.sessions[index].details = Some(SessionDetails {
        branch: Some(branch.into()),
        branches: vec![],
        branch_is_current: false,
        changes: None,
    });
    index
}

#[test]
fn feedback_belongs_to_a_recorded_branch_of_the_exact_checkout() {
    let mut app = Workspace::default();
    let owner = session(&mut app, "/repo", "follow-up");
    app.sessions[owner]
        .details
        .as_mut()
        .unwrap()
        .branches
        .push("feature".into());
    let other = session(&mut app, "/repo", "unrelated");
    let worktree = session(&mut app, "/tree", "feature");
    let pr = requested();
    discover_pull_request(&mut app, "/repo", "feature", &pr);
    assert!(
        app.services.reviews.is_empty(),
        "discovery alone supplies the decision"
    );
    assert_eq!(app.requested_changes(&app.sessions[owner]), Some(&pr));
    for i in [other, worktree] {
        assert!(app.requested_changes(&app.sessions[i]).is_none());
        assert!(app.session_pull_request(&app.sessions[i]).is_none());
    }

    let current = GitHubPullRequest {
        number: 43,
        url: "https://github.com/example/repo/pull/43".into(),
        is_draft: true,
        review_decision: None,
        ..pr.clone()
    };
    discover_pull_request(&mut app, "/repo", "follow-up", &current);
    assert_eq!(
        app.session_pull_request(&app.sessions[owner]),
        Some(&current)
    );
    assert_eq!(app.requested_changes(&app.sessions[owner]), Some(&pr));
    app.sessions[owner].details = None;
    assert!(app.requested_changes(&app.sessions[owner]).is_none());
    assert!(app.session_pull_request(&app.sessions[owner]).is_none());
}

#[test]
fn latest_live_decision_wins_even_when_details_arrive_after_rediscovery() {
    let mut app = Workspace::default();
    let owner = session(&mut app, "/repo", "feature");
    let pr = requested();
    let details = |state: &str, decision: &str| -> Box<GitHubPullRequestDetails> {
        serde_json::from_value(json!({"number":pr.number,"title":pr.title,"url":pr.url,"isDraft":false,"body":"","state":state,"author":null,"headRefName":"feature","headRefOid":"head","mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","additions":0,"deletions":0,"changedFiles":0,"reviewDecision":decision})).unwrap()
    };
    let fetch = Task::Details {
        project: "/repo".into(),
        url: pr.url.clone(),
    };
    let complete = |app: &mut Workspace, id, state, decision| {
        app.apply(Event::Completed(
            id,
            Ok(Reply::Task(TaskReply::Details(details(state, decision)))),
        ));
    };

    app.task(fetch.clone());
    let old = app.commands().pop().unwrap().0;
    discover_pull_request(&mut app, "/repo", "feature", &pr);
    complete(&mut app, old, "OPEN", "APPROVED");
    assert_eq!(app.requested_changes(&app.sessions[owner]), Some(&pr));

    for (state, decision, attention) in [
        ("OPEN", "APPROVED", false),
        ("OPEN", "CHANGES_REQUESTED", true),
        ("CLOSED", "CHANGES_REQUESTED", false),
    ] {
        app.task(fetch.clone());
        let request = app.commands().pop().unwrap().0;
        complete(&mut app, request, state, decision);
        assert_eq!(
            app.session_pull_request(&app.sessions[owner]).is_some(),
            state == "OPEN"
        );
        assert_eq!(
            app.requested_changes(&app.sessions[owner]).is_some(),
            attention
        );
    }
    discover_pull_request(&mut app, "/repo", "feature", &pr);
    assert_eq!(
        app.requested_changes(&app.sessions[owner]),
        Some(&pr),
        "reopened"
    );
    app.task(Task::Discover {
        project: "/repo".into(),
        branch: "feature".into(),
    });
    let request = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(
        request,
        Ok(Reply::Task(TaskReply::Discovered(None))),
    ));
    assert!(app.requested_changes(&app.sessions[owner]).is_none());
}
