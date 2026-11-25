use super::*;
use caelum_protocol::{
    GitHubDiffSide, GitHubPendingReviewComment, GitHubReviewDraft, GitHubReviewEvent,
};
use std::fs;

const URL: &str = "https://git.example.com/team/project/pull/17";
const HEAD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BASE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture(tempfile::TempDir);
impl Fixture {
    fn new() -> Self {
        let f = Self(tempfile::tempdir().unwrap());
        crate::test_support::write_executable(
            &f.0.path().join("gh"),
            r#"#!/bin/sh
set -eu
test "$1 $2 $3" = "api --hostname git.example.com"
shift 3
case "$*" in
    "--method GET repos/team/project/pulls/17") cat pr.json ;;
    "--method POST repos/team/project/pulls/17/reviews --input -") cat > sent.json; echo '{"id":17}' ;;
    "graphql --input -")
        request=$(cat)
        case "$request" in
            *"mutation("*) printf '%s' "$request" > sent.json; cat result.json ;;
            *) cat thread.json ;;
        esac ;;
    *) exit 1 ;;
esac
"#,
        );
        f.set(
            "pr.json",
            json!({"html_url":URL,"head":{"sha":HEAD},"base":{"sha":BASE},"state":"open"}),
        );
        f.thread(URL, HEAD);
        f
    }
    fn set(&self, file: &str, value: Value) {
        fs::write(self.0.path().join(file), value.to_string()).unwrap();
    }
    fn thread(&self, url: &str, head: &str) {
        self.set(
            "thread.json",
            json!({"data":{"node":{"pullRequest":{"url":url,"headRefOid":head,"baseRefOid":BASE,"state":"OPEN"}}}}),
        );
    }
    fn write(&self, action: &GitHubReviewWrite) -> Result<GitHubReviewWriteResult, GitHubError> {
        run(
            &self.0.path().join("gh"),
            self.0.path(),
            URL,
            HEAD,
            BASE,
            action,
        )
    }
    fn sent(&self) -> Value {
        serde_json::from_slice(&fs::read(self.0.path().join("sent.json")).unwrap()).unwrap()
    }
}

#[test]
fn review_posts_one_json_batch_at_the_displayed_commit() {
    let f = Fixture::new();
    let draft = GitHubReviewDraft {
        event: GitHubReviewEvent::RequestChanges,
        body: "Consider both sides.".into(),
        comments: vec![
            GitHubPendingReviewComment {
                path: "src/new name.rs".into(),
                line: 3,
                side: GitHubDiffSide::Left,
                body: "@file\n\"old\"".into(),
            },
            GitHubPendingReviewComment {
                path: "src/new name.rs".into(),
                line: 8,
                side: GitHubDiffSide::Right,
                body: "{owner} 'new'".into(),
            },
        ],
    };
    assert!(matches!(
        f.write(&GitHubReviewWrite::Submit(draft)).unwrap(),
        GitHubReviewWriteResult::Submitted
    ));
    assert_eq!(
        f.sent(),
        json!({"event":"REQUEST_CHANGES", "body":"Consider both sides.", "commit_id":HEAD, "comments":[
            {"path":"src/new name.rs","line":3,"side":"LEFT","body":"@file\n\"old\""},
            {"path":"src/new name.rs","line":8,"side":"RIGHT","body":"{owner} 'new'"}
        ]})
    );
}

#[test]
fn thread_writes_send_the_original_id_and_use_github_confirmation() {
    let f = Fixture::new();
    let body = "Looks good.\nThanks!";
    f.set(
        "result.json",
        json!({"data":{"addPullRequestReviewThreadReply":{"comment":{
            "id":"comment", "url":URL, "body":body, "createdAt":"2026-01-01T00:00:00Z"
        }}}}),
    );
    assert!(
        matches!(f.write(&GitHubReviewWrite::Reply { thread_id:"thread".into(), body:body.into() }).unwrap(),
        GitHubReviewWriteResult::Replied(c) if c.id == "comment")
    );
    assert_eq!(f.sent()["variables"], json!({"id":"thread","body":body}));
    for (resolved, op) in [
        (true, "resolveReviewThread"),
        (false, "unresolveReviewThread"),
    ] {
        f.set("result.json", json!({"data":{(op):{"thread":{"id":"thread", "isResolved":resolved, "viewerCanResolve":!resolved, "viewerCanUnresolve":resolved}}}}));
        assert!(
            matches!(f.write(&GitHubReviewWrite::SetResolved { thread_id:"thread".into(), resolved }).unwrap(),
            GitHubReviewWriteResult::Resolution { resolved: actual, can_unresolve, .. } if actual == resolved && can_unresolve == resolved)
        );
        assert!(f.sent()["query"]
            .as_str()
            .unwrap()
            .contains(&format!(" {op}(")));
    }
}

#[test]
fn a_wrong_thread_stale_head_or_closed_pr_never_reaches_the_write() {
    let f = Fixture::new();
    let resolve = GitHubReviewWrite::SetResolved {
        thread_id: "thread".into(),
        resolved: true,
    };
    f.thread("https://git.example.com/team/project/pull/18", HEAD);
    assert!(f
        .write(&resolve)
        .unwrap_err()
        .to_string()
        .contains("different pull request"));
    f.thread(URL, "other");
    assert!(f
        .write(&resolve)
        .unwrap_err()
        .to_string()
        .contains("head changed"));
    let approve = GitHubReviewWrite::Submit(GitHubReviewDraft {
        event: GitHubReviewEvent::Approve,
        ..Default::default()
    });
    for (head, base, state, expected) in [
        (HEAD, BASE, "closed", "no longer open"),
        ("other", BASE, "open", "head changed"),
        (HEAD, "other", "open", "base changed"),
    ] {
        f.set(
            "pr.json",
            json!({"html_url":URL,"head":{"sha":head},"base":{"sha":base},"state":state}),
        );
        assert!(f
            .write(&approve)
            .unwrap_err()
            .to_string()
            .contains(expected));
    }
    assert!(!f.0.path().join("sent.json").exists());
}
