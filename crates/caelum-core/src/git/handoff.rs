use super::GitClient;
use crate::remote::{valid_branch, PublishedBase};
use std::{process::Stdio, time::Duration};

impl GitClient {
    pub(crate) fn checkout_branch(&self, project: &str) -> Result<String, String> {
        let branch = self
            .branch_context(project)
            .map_err(|e| e.to_string())?
            .and_then(|ctx| ctx.branch)
            .ok_or("Check out a named branch in this session’s worktree, then retry.")?;
        if !valid_branch(&branch) {
            return Err("This branch name is not supported by VM checkouts.".into());
        }
        Ok(branch)
    }

    /// Read origin without fetching into, or publishing anything from, the worktree.
    pub(crate) async fn handoff_base(&self, project: &str) -> Result<PublishedBase, String> {
        let branch = self.checkout_branch(project)?;
        let reference = format!("refs/heads/{branch}");
        let mut cmd = tokio::process::Command::new(&self.executable);
        cmd.current_dir(project)
            .args(["ls-remote", "--exit-code", "--refs", "origin", &reference])
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(20), cmd.output())
            .await
            .map_err(|_| "Timed out checking the session branch on origin.")?
            .map_err(|_| "Could not run Git to check the session branch.")?;
        if output.status.code() == Some(2) {
            return Err(format!(
                "Publish {branch} to origin before continuing on a VM."
            ));
        }
        if !output.status.success() {
            return Err(
                "Could not read the session branch from origin. Check Git access and retry.".into(),
            );
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let tip = text
            .lines()
            .find_map(|l| {
                l.split_once('\t')
                    .filter(|(_, r)| *r == reference)
                    .map(|(oid, _)| oid)
            })
            .filter(|v| matches!(v.len(), 40 | 64) && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or("Origin did not return the session branch.")?;
        Ok(PublishedBase {
            branch,
            tip: tip.into(),
        })
    }
}
