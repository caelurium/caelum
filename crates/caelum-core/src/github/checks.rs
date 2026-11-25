use super::*;
use caelum_protocol::GitHubCheckLogs;
use reqwest::Url;

const MAX_LOG_BYTES: usize = 48 * 1024;

pub fn parse_actions_run(details_url: &str) -> Option<(u64, Option<u64>)> {
    let parsed = Url::parse(details_url).ok()?;
    let segments: Vec<_> = parsed.path_segments()?.collect();
    let runs_pos = segments
        .windows(2)
        .position(|pair| pair == ["actions", "runs"])?;
    let run_id = segments.get(runs_pos + 2)?.parse::<u64>().ok()?;
    let job_id = segments.get(runs_pos + 3..).and_then(|tail| {
        tail.windows(2)
            .find(|pair| matches!(pair[0], "job" | "jobs"))
            .and_then(|pair| pair[1].parse::<u64>().ok())
    });
    Some((run_id, job_id))
}

pub fn pull_request_check_logs(
    project_path: impl AsRef<Path>,
    url: &str,
    head_oid: &str,
    check_name: &str,
    details_url: &str,
) -> Result<GitHubCheckLogs, GitHubError> {
    check_logs(
        &gh_executable(),
        project_path.as_ref(),
        url,
        head_oid,
        check_name,
        details_url,
    )
}

pub(super) fn check_logs(
    executable: &(impl Gh + ?Sized),
    project_path: &Path,
    url: &str,
    head_oid: &str,
    check_name: &str,
    details_url: &str,
) -> Result<GitHubCheckLogs, GitHubError> {
    let pr = pull_request_url(url)?;
    validate_head(head_oid)?;
    let parsed_details = Url::parse(details_url).map_err(|_| GitHubError::InvalidUrl)?;
    if parsed_details.host_str() != pr.host_str() {
        return Err(GitHubError::Command(
            "Check details URL does not match the pull request host.".into(),
        ));
    }
    let (run_id, job_id) = parse_actions_run(details_url).ok_or_else(|| {
        GitHubError::Command(
            "Failure logs are only available for GitHub Actions workflow runs.".into(),
        )
    })?;
    let segments: Vec<_> = pr.path_segments().ok_or(GitHubError::InvalidUrl)?.collect();
    let repo = format!("{}/{}", segments[0], segments[1]);
    let project = executable.project(project_path)?;
    let mut command = github_command(executable, project.path());
    command.args(["run", "view", &run_id.to_string(), "--repo", &repo]);
    if let Some(job) = job_id {
        command.args(["--job", &job.to_string()]);
    }
    command.arg("--log-failed");
    let output = executable.output(command, Duration::from_secs(20))?;
    let raw = String::from_utf8_lossy(&output);
    let (excerpt, truncated) = tail_utf8(&raw, MAX_LOG_BYTES);
    Ok(GitHubCheckLogs {
        run_id,
        job_id,
        check_name: check_name.to_owned(),
        excerpt,
        truncated,
        url: details_url.to_owned(),
    })
}

fn tail_utf8(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_owned(), false);
    }
    let mut start = text.len() - max_bytes;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if let Some(newline) = text[start..].find('\n') {
        start += newline + 1;
    }
    (text[start..].to_owned(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_github_actions_run_and_job_urls() {
        assert_eq!(
            parse_actions_run("https://github.com/caelurium/caelum/actions/runs/123456/job/7890"),
            Some((123456, Some(7890)))
        );
        assert_eq!(
            parse_actions_run("https://github.com/caelurium/caelum/actions/runs/42"),
            Some((42, None))
        );
        assert_eq!(parse_actions_run("https://ci.example.com/build/99"), None);
    }

    #[test]
    fn tail_utf8_preserves_utf8_and_line_boundaries() {
        let input = "line-1\nline-2\ncafé-line-3\n";
        let (tail, truncated) = tail_utf8(input, 16);
        assert!(truncated);
        assert_eq!(tail, "café-line-3\n");
    }
}
