//! Session discovery order is independent of navigation and streamed token arrival.
use super::{Session, Task, Workspace};
use serde::{Deserialize, Serialize};

mod active;
mod rows;
pub use active::OverviewActivity;
pub use rows::FeedSession;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionScope {
    #[default]
    AllProjects,
    CurrentProject,
}

impl Workspace {
    pub fn discover_open_sessions(&mut self) {
        let scopes: Vec<_> = self
            .projects
            .iter()
            .flat_map(|project| {
                super::Provider::ALL.map(|provider| super::Scope {
                    project: project.path.clone(),
                    provider,
                })
            })
            .collect();
        for scope in scopes {
            self.discover_sessions(scope);
        }
    }

    /// A VM has its own checkout, but its feed scope belongs to the source project group.
    pub fn feed_project<'a>(&'a self, filter: Option<&'a str>) -> Option<&'a str> {
        filter
            .or_else(|| {
                if self.preferences.session_scope != SessionScope::CurrentProject {
                    return None;
                }
                self.active_project.as_deref().or_else(|| {
                    let id = self.remote.saved.selected.as_ref()?;
                    Some(self.remote.saved.checkouts.get(id)?.source.project.as_str())
                })
            })
            .map(|p| self.group_path(p))
    }

    /// Shared scope for the session feed and its pull requests, including linked checkouts.
    pub fn feed_includes_project(&self, path: &str, project: Option<&str>) -> bool {
        self.project_is_open(path)
            && match self.feed_project(project) {
                Some(project) => self.group_path(path) == project,
                None => self.preferences.session_scope == SessionScope::AllProjects,
            }
    }

    /// Refresh even when Git's branch is unchanged, without changing session ownership.
    pub fn refresh_feed_pull_requests(&mut self, project: Option<&str>) {
        let paths: Vec<_> = self
            .projects
            .iter()
            .filter(|p| self.feed_includes_project(&p.path, project))
            .map(|p| p.path.clone())
            .collect();
        for path in paths {
            let mut branches: std::collections::BTreeSet<_> =
                self.services.pull_request_branches(&path).collect();
            if let Some(branch) = self
                .services
                .checkouts
                .get(&path)
                .and_then(|c| c.git.as_ref())
                .and_then(|g| g.branch.clone())
            {
                branches.insert(branch);
            }
            for session in self
                .sessions
                .iter()
                .filter(|s| !s.forgotten() && s.scope.project == path)
            {
                if let Some(branch) = self.session_branch(session) {
                    branches.insert(branch.to_owned());
                }
                if let Some(details) = &session.details {
                    branches.extend(details.branch.iter().cloned());
                    branches.extend(details.branches.iter().cloned());
                }
            }
            self.task(Task::Git {
                project: path.clone(),
            });
            for branch in branches {
                self.task(Task::Discover {
                    project: path.clone(),
                    branch,
                });
            }
        }
    }

    /// Return stable local identities, including linked checkouts of each project.
    /// A temporary project filter overrides the saved scope without changing it.
    pub fn session_feed(&self, project: Option<&str>, query: &str, archived: bool) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        let mut indices: Vec<_> = self
            .matching_local_sessions(
                |path| self.feed_includes_project(path, project),
                &query,
                archived,
            )
            .map(|(index, _)| index)
            .collect();
        indices
            .sort_by_key(|&index| (std::cmp::Reverse(self.sessions[index].last_activity), index));
        indices
    }

    fn matching_local_sessions<'a>(
        &'a self,
        includes: impl Fn(&str) -> bool + 'a,
        query: &'a str,
        archived: bool,
    ) -> impl Iterator<Item = (usize, &'a Session)> + 'a {
        self.sessions
            .iter()
            .enumerate()
            .filter(move |(_, session)| {
                !session.forgotten()
                    && session.archived() == archived
                    && includes(&session.scope.project)
                    && (query.is_empty()
                        || format!(
                            "{} {} {} {} {} {}",
                            session.title,
                            session.scope.provider.as_str(),
                            session.scope.project,
                            self.projects
                                .iter()
                                .find(|p| p.path == self.group_path(&session.scope.project))
                                .map_or("", |p| &p.name),
                            self.session_branch(session).unwrap_or_default(),
                            self.session_worktree(session).unwrap_or_default(),
                        )
                        .to_lowercase()
                        .contains(query))
            })
    }

    pub fn session_branch<'a>(&'a self, session: &'a Session) -> Option<&'a str> {
        if !session.archived() {
            if let Some(git) = self
                .services
                .checkouts
                .get(&session.scope.project)
                .and_then(|checkout| checkout.git.as_ref())
            {
                return git.branch.as_deref();
            }
        }
        session
            .details
            .as_ref()
            .and_then(|d| d.branch.as_deref())
            .or_else(|| {
                self.services
                    .checkouts
                    .get(&session.scope.project)?
                    .git
                    .as_ref()?
                    .branch
                    .as_deref()
            })
            .or_else(|| {
                self.services
                    .worktrees(&session.scope.project)?
                    .worktrees
                    .iter()
                    .find(|tree| tree.project.path == session.scope.project)?
                    .branch
                    .as_deref()
            })
            .or_else(|| self.native_checkout(&session.scope.project)?.branch())
    }

    pub fn session_worktree<'a>(&'a self, session: &'a Session) -> Option<&'a str> {
        let path = &session.scope.project;
        if let Some(checkout) = self.native_checkout(path) {
            return Some(checkout.title());
        }
        if self.group_path(path) == path {
            return None;
        }
        self.services
            .worktrees(path)
            .and_then(|trees| trees.worktrees.iter().find(|t| t.project.path == *path))
            .and_then(|tree| tree.label.as_deref())
            .or_else(|| path.rsplit('/').next())
    }
}

#[cfg(test)]
mod tests;
