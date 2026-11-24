use super::GitClient;
use crate::remote::{valid_branch, BranchPublication, PublishedBase, Source};
use std::{process::Stdio, time::Duration};

impl GitClient {
    pub(crate) fn checkout_branch(&self, project: &str) -> Result<String, String> {
        self.local_branch(project)?.ok_or_else(|| {
            "Check out a named branch in this session’s worktree, then retry.".into()
        })
    }

    fn local_branch(&self, project: &str) -> Result<Option<String>, String> {
        let branch = self
            .branch_context(project)
            .map_err(|e| e.to_string())?
            .ok_or("Open a Git worktree before creating a VM.")?
            .branch;
        if branch.as_deref().is_some_and(|b| !valid_branch(b)) {
            return Err("This branch name is not supported by VM checkouts.".into());
        }
        Ok(branch)
    }

    /// Read origin without fetching into, or publishing anything from, the worktree.
    pub(crate) async fn handoff_base(&self, project: &str) -> Result<PublishedBase, String> {
        let branch = self.checkout_branch(project)?;
        self.published_base(project, branch).await
    }

    async fn published_base(&self, project: &str, branch: String) -> Result<PublishedBase, String> {
        let tip = self
            .origin_tip(project, &branch)
            .await?
            .ok_or_else(|| format!("Publish {branch} to origin before continuing on a VM."))?;
        Ok(PublishedBase { branch, tip })
    }

    pub(crate) async fn destination_base(
        &self,
        project: &str,
        publication: Option<&(BranchPublication, String)>,
    ) -> Result<PublishedBase, String> {
        if let Some((publication, branch)) = publication {
            if valid_branch(branch)
                && self.local_branch(project)?.as_deref() == publication.local_branch()
            {
                return self.published_base(project, branch.clone()).await;
            }
        }
        self.handoff_base(project).await
    }

    pub(crate) async fn vm_base(
        &self,
        project: &str,
    ) -> Result<(PublishedBase, Option<BranchPublication>), String> {
        let publish = match self.local_branch(project)? {
            Some(branch) if branch == "main" => BranchPublication::OnMain,
            Some(branch) => match self.origin_tip(project, &branch).await? {
                Some(tip) => return Ok((PublishedBase { branch, tip }, None)),
                None => BranchPublication::Named(branch),
            },
            None => BranchPublication::Detached,
        };
        let tip = self
            .origin_tip(project, "main")
            .await?
            .ok_or("No published origin/main exists to start the VM branch from.")?;
        Ok((
            PublishedBase {
                branch: "main".into(),
                tip,
            },
            Some(publish),
        ))
    }

    async fn origin_tip(&self, project: &str, branch: &str) -> Result<Option<String>, String> {
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
            return Ok(None);
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
        Ok(Some(tip.into()))
    }

    pub(crate) async fn ensure_vm_branch(
        &self,
        source: &Source,
        token: &str,
        create_ref: impl FnOnce(&Source, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        let changed = "The checkout or its published branch changed. Reopen VM setup.";
        if !self
            .github_origin(&source.project)?
            .eq_ignore_ascii_case(&source.repository)
        {
            return Err(changed.into());
        }
        let tip = source.published_tip.as_ref().ok_or(changed)?;
        let Some(publish) = &source.publish else {
            let actual = self.handoff_base(&source.project).await?;
            return if actual.tip == *tip && actual.branch == source.base {
                Ok(())
            } else {
                Err(changed.into())
            };
        };
        if !source.valid_plan_branch(&source.base)
            || self.local_branch(&source.project)?.as_deref() != publish.local_branch()
            || source.base != publish.destination(token)
            || source.base == "main"
        {
            return Err(changed.into());
        }
        if let Some(existing) = self.origin_tip(&source.project, &source.base).await? {
            return if existing == *tip {
                Ok(())
            } else {
                Err(format!(
                    "origin/{} already points to a different commit. Reopen VM setup.",
                    source.base
                ))
            };
        }
        if self.origin_tip(&source.project, "main").await?.as_ref() != Some(tip) {
            return Err(changed.into());
        }
        let result = create_ref(source, tip);
        if self
            .origin_tip(&source.project, &source.base)
            .await?
            .as_ref()
            == Some(tip)
        {
            Ok(())
        } else {
            Err(result.err().unwrap_or_else(|| {
                format!("Could not verify origin/{} after creating it.", source.base)
            }))
        }
    }
}

#[cfg(test)]
mod tests;
