use super::{check_base, check_target, COMMENT_FIELDS};
use crate::github::{
    gh_executable, github_command, pull_request_url, validate_head, Gh, GitHubError,
};
use caelum_protocol::{GitHubReviewWrite, GitHubReviewWriteResult};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

const TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn write_review(
    project: &str,
    url: &str,
    head: &str,
    base: &str,
    action: &GitHubReviewWrite,
) -> Result<GitHubReviewWriteResult, GitHubError> {
    run(
        &gh_executable(),
        Path::new(project),
        url,
        head,
        base,
        action,
    )
}

pub(in crate::github) fn run(
    gh: &(impl Gh + ?Sized),
    project: &Path,
    url: &str,
    head: &str,
    base: &str,
    action: &GitHubReviewWrite,
) -> Result<GitHubReviewWriteResult, GitHubError> {
    validate_head(head)?;
    validate_head(base)?;
    let parsed = pull_request_url(url)?;
    let project = gh.project(project)?;
    let project = project.path();
    let api = || {
        let mut cmd = github_command(gh, project);
        cmd.args(["api", "--hostname", parsed.host_str().unwrap()]);
        cmd
    };
    let graphql = |query: &str, variables: Value| -> Result<Value, GitHubError> {
        let mut cmd = api();
        cmd.args(["graphql", "--input", "-"]);
        let payload = json!({ "query": query, "variables": variables });
        let response: Value = serde_json::from_slice(&gh.json_input(cmd, &payload, TIMEOUT)?)?;
        if let Some(errors) = response
            .get("errors")
            .filter(|e| e.as_array().is_none_or(|v| !v.is_empty()))
        {
            return Err(GitHubError::Command(format!(
                "GitHub review request failed: {errors}"
            )));
        }
        Ok(response)
    };
    let thread_id = match action {
        GitHubReviewWrite::Submit(draft) => {
            if !draft.can_submit() {
                return Err(GitHubError::Command(
                    "The review is missing a summary or a valid inline comment.".into(),
                ));
            }
            let parts: Vec<_> = parsed.path_segments().unwrap().collect();
            let endpoint = format!("repos/{}/{}/pulls/{}", parts[0], parts[1], parts[3]);
            let mut lookup = api();
            lookup.args(["--method", "GET", &endpoint]);
            let pr: Value = serde_json::from_slice(&gh.output(lookup, TIMEOUT)?)?;
            check_target(gh, project, &pr, url, head, true)?;
            check_base(&pr["base"]["sha"], base)?;
            if pr["state"] != "open" {
                return Err(GitHubError::Command(
                    "This pull request is no longer open.".into(),
                ));
            }
            let mut cmd = api();
            cmd.args([
                "--method",
                "POST",
                &format!("{endpoint}/reviews"),
                "--input",
                "-",
            ]);
            let payload = json!({"commit_id": head, "event": draft.event, "body": draft.body, "comments": draft.comments});
            gh.json_input(cmd, &payload, TIMEOUT)?;
            return Ok(GitHubReviewWriteResult::Submitted);
        }
        GitHubReviewWrite::Reply { thread_id, body } if !body.trim().is_empty() => thread_id,
        GitHubReviewWrite::SetResolved { thread_id, .. } => thread_id,
        _ => return Err(GitHubError::Command("Enter a reply first.".into())),
    };
    let node = graphql(
        "query($id: ID!) { node(id: $id) { ... on PullRequestReviewThread { pullRequest { url headRefOid baseRefOid state } } } }",
        json!({"id": thread_id}),
    )?;
    let pr = &node["data"]["node"]["pullRequest"];
    check_target(gh, project, pr, url, head, false)?;
    check_base(&pr["baseRefOid"], base)?;
    if pr["state"] != "OPEN" {
        return Err(GitHubError::Command(
            "This pull request is no longer open.".into(),
        ));
    }
    match action {
        GitHubReviewWrite::Reply { body, .. } => {
            let result = graphql(&format!("mutation($id: ID!, $body: String!) {{ addPullRequestReviewThreadReply(input: {{ pullRequestReviewThreadId: $id, body: $body }}) {{ comment {{ {COMMENT_FIELDS} }} }} }}"), json!({"id":thread_id, "body":body}))?;
            Ok(GitHubReviewWriteResult::Replied(serde_json::from_value(
                result["data"]["addPullRequestReviewThreadReply"]["comment"].clone(),
            )?))
        }
        GitHubReviewWrite::SetResolved { resolved, .. } => {
            let op = if *resolved {
                "resolveReviewThread"
            } else {
                "unresolveReviewThread"
            };
            let result = graphql(&format!("mutation($id: ID!) {{ {op}(input: {{ threadId: $id }}) {{ thread {{ id isResolved viewerCanResolve viewerCanUnresolve }} }} }}"), json!({"id":thread_id}))?;
            let thread = &result["data"][op]["thread"];
            if thread["id"].as_str() != Some(thread_id)
                || thread["isResolved"].as_bool() != Some(*resolved)
            {
                return Err(GitHubError::Command(
                    "GitHub did not confirm the thread resolution.".into(),
                ));
            }
            Ok(GitHubReviewWriteResult::Resolution {
                resolved: *resolved,
                can_resolve: thread["viewerCanResolve"] == true,
                can_unresolve: thread["viewerCanUnresolve"] == true,
            })
        }
        GitHubReviewWrite::Submit(_) => unreachable!(),
    }
}

#[cfg(all(test, unix))]
mod tests;
