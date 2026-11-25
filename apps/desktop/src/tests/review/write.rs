use super::*;
use caelum_core::application::{Command, Event, Reply, Task, TaskReply};
use caelum_protocol::{
    GitHubDiffSide, GitHubReviewEvent, GitHubReviewWrite, GitHubReviewWriteResult,
};

fn write_request(rt: &mut Runtime<Workspace>) -> (u64, GitHubReviewWrite) {
    let l = rt.state.live.as_mut().unwrap();
    l.app
        .commands()
        .into_iter()
        .find_map(|(id, c)| match c {
            Command::Task(Task::WriteReview {
                project,
                url,
                head,
                base,
                action,
            }) => {
                assert_eq!(Some(&(project, url)), l.panels.review.as_ref());
                assert_eq!(head, "a".repeat(40));
                assert_eq!(base, "b".repeat(40));
                Some((id, action))
            }
            _ => None,
        })
        .expect("a write for the displayed revision")
}

#[test]
fn gutter_comments_batch_on_both_sides_and_retry_with_the_summary_intact() {
    let mut rt = design_runtime("review", (1440.0, 1000.0));
    for (gutter, body) in [
        ("df0.o2/gutter/plus", "Keep the old behavior."),
        ("df0.n2/line/gutter/plus", "Cover the new behavior."),
    ] {
        let (x, y) = rt.find(gutter).unwrap();
        rt.pointer_move(x, y);
        advance(&mut rt, 0.2);
        click(&mut rt, gutter);
        assert!(
            rt.find("review-line-input").is_some(),
            "gutter {gutter} at ({x}, {y}) picked {:?}",
            rt.element_at(x, y)
        );
        fill(&mut rt, "review-line-input", body);
        click(&mut rt, "review-queue-line");
    }
    assert_eq!(
        element(&rt, "review-composer-toggle").label.as_deref(),
        Some("Review (2)")
    );
    click(&mut rt, "review-tab-0");
    click(&mut rt, "review-composer-toggle");
    click(&mut rt, "review-event-RequestChanges");
    assert!(!element(&rt, "review-submit").focusable);
    fill(
        &mut rt,
        "review-summary-input",
        "Please address both cases.",
    );
    click(&mut rt, "review-submit");
    let (id, GitHubReviewWrite::Submit(draft)) = write_request(&mut rt) else {
        panic!("expected review")
    };
    assert_eq!(draft.event, GitHubReviewEvent::RequestChanges);
    assert_eq!(draft.body, "Please address both cases.");
    assert_eq!(draft.comments.len(), 2);
    for (c, side) in draft
        .comments
        .iter()
        .zip([GitHubDiffSide::Left, GitHubDiffSide::Right])
    {
        assert_eq!(c.path, "src/workspace.rs");
        assert_eq!((c.line, c.side), (2, side));
    }
    assert!(!element(&rt, "review-composer-toggle").focusable);
    rt.update(|s, _| {
        s.live
            .as_mut()
            .unwrap()
            .apply(Event::Completed(id, Err("Offline".into())))
    });
    advance(&mut rt, 0.2);
    click(&mut rt, "review-composer-toggle");
    click(&mut rt, "review-submit");
    let (_, GitHubReviewWrite::Submit(retry)) = write_request(&mut rt) else {
        panic!("expected retry")
    };
    assert_eq!(retry, draft);
}

#[test]
fn reply_resolve_and_reopen_a_thread_at_its_diff_location() {
    let mut rt = design_runtime("review", (800.0, 620.0));
    click(&mut rt, "thread-reply-thread-0");
    advance(&mut rt, 0.3);
    let input = element(&rt, "thread-reply-input");
    assert!(input.visible[3] >= input.rect[3] - 1.0);
    fill(&mut rt, "thread-reply-input", "Addressed in this revision.");
    click(&mut rt, "thread-reply-submit");
    let (id, GitHubReviewWrite::Reply { thread_id, body }) = write_request(&mut rt) else {
        panic!("expected reply")
    };
    assert_eq!(thread_id, "thread-0");
    assert_eq!(body, "Addressed in this revision.");
    let reply = serde_json::from_value(serde_json::json!({"id":"new-comment","url":"https://git.example.com/team/repo/pull/3","body":body,"createdAt":"2026-01-01T00:00:00Z"})).unwrap();
    rt.update(|s, _| {
        s.live.as_mut().unwrap().apply(Event::Completed(
            id,
            Ok(Reply::Task(TaskReply::ReviewWritten(
                GitHubReviewWriteResult::Replied(reply),
            ))),
        ))
    });
    advance(&mut rt, 0.2);
    assert!(rt.find("select-comment-new-comment").is_some());
    for resolve in [true, false] {
        if !resolve {
            reveal(&mut rt, "thread-thread-0");
            click(&mut rt, "thread-thread-0");
        }
        reveal(&mut rt, "thread-resolve-thread-0");
        click(&mut rt, "thread-resolve-thread-0");
        let (
            id,
            GitHubReviewWrite::SetResolved {
                thread_id,
                resolved,
            },
        ) = write_request(&mut rt)
        else {
            panic!("expected resolution")
        };
        assert_eq!(thread_id, "thread-0");
        assert_eq!(resolved, resolve);
        rt.update(|s, _| {
            s.live.as_mut().unwrap().apply(Event::Completed(
                id,
                Ok(Reply::Task(TaskReply::ReviewWritten(
                    GitHubReviewWriteResult::Resolution {
                        resolved,
                        can_resolve: !resolved,
                        can_unresolve: resolved,
                    },
                ))),
            ))
        });
        advance(&mut rt, 0.2);
    }
}
