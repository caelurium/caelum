use super::{Session, Workspace};
use crate::application::{Attachment, Provider, Scope, Task};
use caelum_protocol::{GitHubCheckLogs, GitHubPullRequest, SessionOrigin};

pub(super) struct PendingCiFix {
    pub project: String,
    pub pr_url: String,
    pub pr_number: u64,
    pub head_oid: String,
    pub check_name: String,
    pub run_id: Option<u64>,
    pub parent_session_id: Option<String>,
    pub provider: Provider,
}

impl Workspace {
    /// Surface feedback only on branches attributed to this session.
    pub fn requested_changes(&self, session: &Session) -> Option<&GitHubPullRequest> {
        let details = session.details.as_ref()?;
        details
            .branch
            .iter()
            .chain(details.branches.iter().rev())
            .find_map(|branch| {
                self.services
                    .requested_changes(&session.scope.project, branch)
            })
    }

    /// Open pull request on a branch attributed to this session, never borrowed
    /// from another session sharing the same checkout.
    pub fn session_pull_request(&self, session: &Session) -> Option<&GitHubPullRequest> {
        let details = session.details.as_ref()?;
        details
            .branch
            .iter()
            .chain(details.branches.iter().rev())
            .find_map(|branch| {
                self.services
                    .branch_pull_request(&session.scope.project, branch)
            })
    }

    pub(super) fn discover_session_pull_requests(&mut self, index: usize) {
        let Some(session) = self.sessions.get(index) else {
            return;
        };
        let Some(details) = &session.details else {
            return;
        };
        let project = session.scope.project.clone();
        let branches: std::collections::BTreeSet<_> = details
            .branch
            .iter()
            .chain(details.branches.iter())
            .cloned()
            .collect();
        for branch in branches {
            self.task(crate::application::Task::Discover {
                project: project.clone(),
                branch,
            });
        }
    }

    /// Deduplicated open pull requests across all open projects and linked checkouts.
    pub fn open_pull_requests(&self) -> Vec<(&str, &GitHubPullRequest)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for (project, checkout) in &self.services.checkouts {
            if !self.project_is_open(project) {
                continue;
            }
            for pr in &checkout.pull_requests {
                if seen.insert(pr.url.as_str()) {
                    result.push((project.as_str(), pr));
                }
            }
        }
        result
    }

    /// Restore the last overview/diff while GitHub refreshes current metadata.
    pub fn open_review(&mut self, project: String, url: String) {
        if self
            .services
            .reviews
            .get(&(project.clone(), url.clone()))
            .is_none_or(|review| review.details().is_none())
        {
            self.task(crate::application::Task::ReviewCache {
                project: project.clone(),
                url: url.clone(),
            });
        }
        self.task(crate::application::Task::Details { project, url });
    }

    /// Find the conversation associated with the reviewed source branch, including
    /// its recorded history in linked checkouts. The checkout's current Git branch
    /// alone is not evidence that a session worked on the pull request.
    pub fn pull_request_session(&self, project: &str, url: &str) -> Option<usize> {
        let head = &self
            .services
            .reviews
            .get(&(project.into(), url.into()))?
            .details()?
            .head_ref_name;
        if head.is_empty() {
            return None;
        }
        let group = self.group_path(project);
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                !session.forgotten()
                    && !session.deleting()
                    && !session.archiving()
                    && self.group_path(&session.scope.project) == group
                    && self
                        .projects
                        .iter()
                        .any(|p| p.path == session.scope.project)
                    && session.details.as_ref().is_some_and(|details| {
                        details.branch.as_ref() == Some(head) || details.branches.contains(head)
                    })
            })
            .max_by_key(|(index, session)| {
                (
                    !session.archived(),
                    self.selected == Some(*index),
                    session.last_activity,
                    std::cmp::Reverse(*index),
                )
            })
            .map(|(index, _)| index)
    }

    pub fn checkout_has_running_turn(&self, project: &str) -> bool {
        self.sessions
            .iter()
            .any(|s| !s.forgotten() && !s.archived() && s.scope.project == project && s.working())
    }

    pub fn ci_fix_child_session(
        &self,
        pr_url: &str,
        head_oid: &str,
        check_name: &str,
    ) -> Option<usize> {
        self.sessions.iter().position(|s| {
            !s.forgotten()
                && !s.archived()
                && matches!(
                    &s.origin,
                    Some(SessionOrigin::PullRequestCheck {
                        pr_url: url,
                        head_oid: oid,
                        check_name: name,
                        ..
                    }) if url == pr_url && oid == head_oid && name == check_name
                )
        })
    }

    pub fn ci_fix_pending(&self, pr_url: &str, check_name: &str) -> bool {
        self.pending_ci_fixes
            .values()
            .any(|p| p.pr_url == pr_url && p.check_name == check_name)
    }

    pub fn origin_parent_session(&self, index: usize) -> Option<usize> {
        let session = self.sessions.get(index)?;
        let parent_id = match &session.origin {
            Some(SessionOrigin::PullRequestCheck {
                parent_session_id: Some(id),
                ..
            }) => id.as_str(),
            _ => return None,
        };
        let group = self.group_path(&session.scope.project);
        self.sessions.iter().enumerate().find_map(|(i, s)| {
            (i != index
                && !s.forgotten()
                && !s.archived()
                && self.group_path(&s.scope.project) == group
                && s.remote_id.as_deref() == Some(parent_id))
            .then_some(i)
        })
    }

    /// Spawn (or focus) a linked child session to fix a failing CI check on a pull request.
    /// Automatically creates an isolated worktree only if a turn is already running on the
    /// target checkout; otherwise reuses the checkout directly without touching the parent draft.
    pub fn fix_failing_check(
        &mut self,
        project: String,
        pr_url: String,
        check_name: String,
        details_url: Option<String>,
    ) -> Option<usize> {
        let details = self
            .services
            .reviews
            .get(&(project.clone(), pr_url.clone()))?
            .details()?
            .clone();
        let head_oid = details.head_ref_oid.clone();
        if let Some(existing) = self.ci_fix_child_session(&pr_url, &head_oid, &check_name) {
            self.select(existing);
            return Some(existing);
        }
        let parent = self.pull_request_session(&project, &pr_url);
        let parent_session_id = parent.and_then(|i| self.sessions[i].remote_id.clone());
        let provider = parent
            .map(|i| self.sessions[i].scope.provider)
            .or_else(|| self.selected.map(|i| self.sessions[i].scope.provider))
            .unwrap_or_else(|| self.default_provider_for(Some(&project)));
        let target_checkout = parent
            .map(|i| self.sessions[i].scope.project.clone())
            .unwrap_or_else(|| project.clone());
        let run_id = details_url
            .as_deref()
            .and_then(crate::github::parse_actions_run)
            .map(|(id, _)| id);

        if let Some(details_url) = details_url
            .filter(|u| crate::github::parse_actions_run(u).is_some())
            .filter(|_| {
                self.services
                    .reviews
                    .get(&(project.clone(), pr_url.clone()))
                    .is_none_or(|r| !r.check_logs.contains_key(&check_name))
            })
        {
            self.task(Task::CheckLogs {
                project: project.clone(),
                url: pr_url.clone(),
                head: head_oid.clone(),
                check_name: check_name.clone(),
                details_url,
            });
        }

        let pending = PendingCiFix {
            project: project.clone(),
            pr_url: pr_url.clone(),
            pr_number: details.pull_request.number,
            head_oid,
            check_name: check_name.clone(),
            run_id,
            parent_session_id,
            provider,
        };

        if self.checkout_has_running_turn(&target_checkout) {
            if self.ci_fix_pending(&pr_url, &check_name) {
                return None;
            }
            let root = self.group_path(&target_checkout).to_owned();
            let base = if details.head_ref_name.is_empty() {
                "HEAD".to_owned()
            } else {
                details.head_ref_name.clone()
            };
            let branch = ci_fix_branch(details.pull_request.number, &check_name);
            let directory = self.preferences.worktree_directory.clone();
            let previous = self.next_request;
            self.task(Task::CreateWorktree {
                project: root,
                branch,
                base,
                directory,
            });
            if self.next_request != previous {
                let req = self.next_request;
                self.launches.remove(&req);
                self.pending_ci_fixes.insert(req, pending);
            }
            return None;
        }

        Some(self.create_ci_fix_session(&target_checkout, pending))
    }

    pub(super) fn complete_ci_fix_worktree(
        &mut self,
        request: u64,
        worktree: &caelum_protocol::GitWorktree,
    ) -> bool {
        let Some(pending) = self.pending_ci_fixes.remove(&request) else {
            return false;
        };
        if !self
            .projects
            .iter()
            .any(|p| p.path == worktree.project.path)
        {
            self.projects.push(worktree.project.clone());
        }
        self.create_ci_fix_session(&worktree.project.path, pending);
        true
    }

    pub(super) fn attach_check_logs_to_child(
        &mut self,
        pr_url: &str,
        head_oid: &str,
        logs: &GitHubCheckLogs,
    ) {
        let Some(index) = self.ci_fix_child_session(pr_url, head_oid, &logs.check_name) else {
            return;
        };
        let path = check_log_attachment_path(&logs.check_name);
        if self.sessions[index]
            .attachments
            .iter()
            .any(|a| a.path == path)
        {
            return;
        }
        self.attach(
            index,
            Attachment::new(path, logs.excerpt.clone(), None, logs.truncated),
        );
    }

    fn create_ci_fix_session(&mut self, checkout: &str, pending: PendingCiFix) -> usize {
        let cached_logs = self
            .services
            .reviews
            .get(&(pending.project.clone(), pending.pr_url.clone()))
            .and_then(|r| r.check_logs.get(&pending.check_name))
            .cloned();
        let scope = Scope {
            project: checkout.to_owned(),
            provider: pending.provider,
        };
        let index = self.new_session(scope);
        let session = &mut self.sessions[index];
        session.title = format!("Fix CI: {}", pending.check_name);
        session.draft = format!(
            "Fix the failing CI check `{}` on pull request #{} ({}).\n\nInspect the attached failure logs, reproduce the failure locally, apply the minimal fix, and verify the relevant checks pass.",
            pending.check_name, pending.pr_number, pending.pr_url
        );
        session.origin = Some(SessionOrigin::PullRequestCheck {
            parent_session_id: pending.parent_session_id,
            project: pending.project,
            pr_url: pending.pr_url,
            pr_number: pending.pr_number,
            head_oid: pending.head_oid,
            check_name: pending.check_name.clone(),
            run_id: pending.run_id,
        });
        if let Some(logs) = cached_logs {
            let path = check_log_attachment_path(&pending.check_name);
            self.attach(
                index,
                Attachment::new(path, logs.excerpt, None, logs.truncated),
            );
        }
        index
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
        if out.len() >= 28 {
            break;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "check".into()
    } else {
        trimmed.to_owned()
    }
}

fn ci_fix_branch(pr_number: u64, check_name: &str) -> String {
    format!("ci-fix-pr-{pr_number}-{}", slug(check_name))
}

fn check_log_attachment_path(check_name: &str) -> String {
    format!("ci-logs/{}.log", slug(check_name))
}

#[cfg(test)]
mod tests;
