use super::*;
use caelum_protocol::{
    GitHubDiffSide, GitHubPendingReviewComment, GitHubReviewDraft, GitHubReviewEvent,
    GitHubReviewWrite, GitHubReviewWriteResult,
};

type Key = (String, String);

fn fixture() -> (Workspace, Key, GitHubReviewDraft) {
    let key: Key = (
        "/checkout".into(),
        "https://git.example.com/project/repo/pull/3".into(),
    );
    let mut app = Workspace::default();
    let r = app.services.reviews.entry(key.clone()).or_default();
    r.set_details(serde_json::from_value(json!({
        "number":3,"title":"Review","url":key.1,"isDraft":false,"state":"OPEN","body":"",
        "headRefName":"feature","baseRefName":"main","headRefOid":"head","baseRefOid":"base",
        "mergeable":"MERGEABLE","mergeStateStatus":"CLEAN","additions":1,"deletions":1,"changedFiles":1
    })).unwrap());
    r.draft = GitHubReviewDraft {
        event: GitHubReviewEvent::RequestChanges,
        body: "Keep the existing behavior.".into(),
        comments: vec![GitHubPendingReviewComment {
            path: "lib.rs".into(),
            side: GitHubDiffSide::Left,
            line: 5,
            body: "This case still matters.".into(),
        }],
    };
    let draft = r.draft.clone();
    (app, key, draft)
}

fn submit(key: &Key, draft: &GitHubReviewDraft) -> Task {
    Task::WriteReview {
        project: key.0.clone(),
        url: key.1.clone(),
        head: "head".into(),
        base: "base".into(),
        action: GitHubReviewWrite::Submit(draft.clone()),
    }
}

fn success() -> Result<Reply, String> {
    Ok(Reply::Task(TaskReply::ReviewWritten(
        GitHubReviewWriteResult::Submitted,
    )))
}

#[test]
fn a_failed_write_keeps_its_draft_and_the_lock_survives_navigation_and_refresh() {
    let (mut app, key, draft) = fixture();
    app.task(submit(&key, &draft));
    let attempt = app.commands().pop().unwrap().0;
    app.services.refresh_review(&key.0, &key.1);
    app.active_project = Some("/another-checkout".into());
    let other = ("/another-checkout".into(), key.1.clone());
    app.services
        .reviews
        .entry(other.clone())
        .or_default()
        .draft
        .body = "Independent".into();
    for task in [
        submit(&key, &draft),
        Task::UpdateBranch {
            project: key.0.clone(),
            url: key.1.clone(),
            head: "head".into(),
        },
    ] {
        app.task(task);
        assert!(app.commands().is_empty());
    }
    app.apply(Event::Completed(attempt, Err("Permission denied".into())));
    let review = &app.services.reviews[&key];
    assert_eq!(review.error.as_deref(), Some("Permission denied"));
    assert_eq!(review.draft, draft);
    app.task(submit(&key, &draft));
    let retry = app.commands().pop().unwrap().0;
    app.apply(Event::Completed(attempt, success()));
    assert!(app.services.review_mutating(&key.0, &key.1));
    app.apply(Event::Completed(retry, success()));
    assert!(!app.services.review_mutating(&key.0, &key.1));
    assert_eq!(
        app.services.reviews[&key].draft,
        GitHubReviewDraft::default()
    );
    assert_eq!(app.services.reviews[&other].draft.body, "Independent");
    assert!(app.commands().iter().any(|(_, c)| matches!(c, Command::Task(Task::Details {project,url}) if (project,url) == (&key.0,&key.1))));
}

#[test]
fn replacing_the_diff_invalidates_pending_lines_and_late_write_results() {
    for change_base in [true, false] {
        for result in [success(), Err("Old failure".into())] {
            let (mut app, key, draft) = fixture();
            app.task(submit(&key, &draft));
            let attempt = app.commands().pop().unwrap().0;
            app.task(Task::Details {
                project: key.0.clone(),
                url: key.1.clone(),
            });
            let refresh = app.commands().pop().unwrap().0;
            let mut details = app.services.reviews[&key].details().unwrap().clone();
            if change_base {
                details.base_ref_oid = "next-base".into()
            } else {
                details.head_ref_oid = "next-head".into()
            }
            app.apply(Event::Completed(
                refresh,
                Ok(Reply::Task(TaskReply::Details(Box::new(details)))),
            ));
            app.commands();
            let r = app.services.reviews.get_mut(&key).unwrap();
            assert!(r.draft.comments.is_empty());
            assert!(r.notice.as_deref().unwrap().contains("diff changed"));
            if change_base {
                r.edit_details(|d| d.base_ref_oid = "base".into());
            }
            r.draft.body = "For the new revision".into();
            assert!(app.services.review_mutating(&key.0, &key.1));
            app.apply(Event::Completed(attempt, result));
            assert!(!app.services.review_mutating(&key.0, &key.1));
            assert_eq!(
                app.services.reviews[&key].draft.body,
                "For the new revision"
            );
            assert!(app.services.reviews[&key].error.is_none());
            assert!(app.commands().is_empty());
        }
    }
}
