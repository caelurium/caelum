use super::{Pending, Workspace};
use crate::application::{Command, Provider, Scope, Task};

#[derive(Clone, Copy)]
pub enum ProjectMove {
    Up,
    Down,
}

impl Workspace {
    fn project_step(&self, path: &str, direction: ProjectMove) -> Option<(usize, usize)> {
        let mut roots = self
            .projects
            .iter()
            .enumerate()
            .filter(|(_, p)| self.group_path(&p.path) == p.path);
        match direction {
            ProjectMove::Up => {
                let (i, _) = roots.rfind(|(_, p)| p.path == path)?;
                Some((i, roots.next_back()?.0))
            }
            ProjectMove::Down => {
                let (i, _) = roots.find(|(_, p)| p.path == path)?;
                Some((i, roots.next()?.0))
            }
        }
    }

    pub fn can_move_project(&self, path: &str, direction: ProjectMove) -> bool {
        self.project_step(path, direction).is_some()
    }

    /// The visible root order is persisted in `projects`; worktrees are grouped by path.
    pub fn move_project(&mut self, path: &str, direction: ProjectMove) {
        if let Some((i, neighbor)) = self.project_step(path, direction) {
            self.projects.swap(i, neighbor);
        }
    }

    pub fn open_project(&mut self, path: String) {
        self.notifications.cancel();
        self.opening = true;
        self.error = None;
        self.latest_open = self.request(Command::Open(path), Pending::Open(None, self.navigation));
    }

    pub fn launch_project(&mut self, path: String, provider: Provider) {
        self.notifications.cancel();
        self.opening = true;
        self.error = None;
        self.latest_open = self.request(
            Command::Open(path),
            Pending::Open(Some(provider), self.navigation),
        );
    }

    fn best_project_session(&self, path: &str) -> Option<usize> {
        let remembered = self.remembered.get(path).copied();
        self.sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                !session.archived()
                    && !session.forgotten()
                    && !session.archiving()
                    && !session.deleting()
                    && (session.scope.project == path
                        || (crate::remote::native::checkout_id(&session.scope.project).is_none()
                            && self.group_path(&session.scope.project) == path))
            })
            .max_by_key(|(index, session)| {
                (
                    session.has_context(),
                    remembered == Some(*index),
                    session.last_activity,
                    *index,
                )
            })
            .map(|(index, _)| index)
    }

    fn new_background_session(&mut self, scope: Scope) -> usize {
        let previous_selected = self.selected;
        let previous_project = self.active_project.clone();
        let index = self.new_session(scope);
        self.selected = previous_selected;
        self.active_project = previous_project;
        index
    }

    /// Restore this project's conversation, including its linked worktrees. Existing
    /// context takes precedence over an empty placeholder created before discovery.
    pub fn select_project(&mut self, path: &str) -> bool {
        if !self.projects.iter().any(|project| project.path == path) {
            return false;
        }
        if let Some(index) = self.best_project_session(path) {
            self.select(index);
        } else {
            self.new_session(Scope {
                project: path.into(),
                provider: self.preferences.default_provider,
            });
        }
        true
    }

    /// Resolve (or create) a session for `path` to display in a split pane without
    /// replacing the primary pane's selected session.
    pub fn project_session_for_split(&mut self, path: &str) -> Option<usize> {
        if !self.projects.iter().any(|project| project.path == path) {
            return None;
        }
        let index = self.best_project_session(path).unwrap_or_else(|| {
            self.new_background_session(Scope {
                project: path.into(),
                provider: self.preferences.default_provider,
            })
        });
        if !self.sessions[index].loaded() {
            self.refresh(index);
        }
        Some(index)
    }

    /// Resolve (or create) a session for an exact checkout/worktree `path` to display
    /// in a split pane without replacing the primary pane's selected session.
    pub fn checkout_session_for_split(&mut self, path: &str) -> Option<usize> {
        if !self.projects.iter().any(|project| project.path == path)
            || self
                .services
                .worktrees(self.group_path(path))
                .is_some_and(|listing| {
                    !listing
                        .worktrees
                        .iter()
                        .any(|tree| tree.project.path == path && tree.available)
                })
        {
            return None;
        }
        let usable = |i: usize| {
            let session = &self.sessions[i];
            session.scope.project == path && !session.archived() && !session.forgotten()
        };
        let index = self
            .remembered_checkouts
            .get(path)
            .or(self.remembered.get(path))
            .copied()
            .filter(|&i| usable(i))
            .or_else(|| (0..self.sessions.len()).rev().find(|&i| usable(i)))
            .unwrap_or_else(|| {
                self.new_background_session(Scope {
                    project: path.into(),
                    provider: self.preferences.default_provider,
                })
            });
        if !self.sessions[index].loaded() {
            self.refresh(index);
        }
        Some(index)
    }

    /// Update `active_project` to match a focused split pane without changing `selected`.
    pub fn focus_project_context(&mut self, path: &str) {
        if self.active_project.as_deref() != Some(path)
            && self.projects.iter().any(|project| project.path == path)
        {
            self.active_project = Some(path.to_owned());
        }
    }

    pub(super) fn select_open_session_or_new(&mut self, preferred_project: &str, fallback: Scope) {
        let preferred_group = self.group_path(preferred_project).to_owned();
        let selected = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                !session.archived()
                    && !session.forgotten()
                    && !session.archiving()
                    && !session.deleting()
                    && self.project_is_open(&session.scope.project)
            })
            .max_by_key(|(index, session)| {
                (
                    session.has_context(),
                    self.group_path(&session.scope.project) == preferred_group,
                    session.scope.project == preferred_project,
                    *index,
                )
            })
            .map(|(index, _)| index);
        if let Some(index) = selected {
            self.select(index);
        } else {
            self.new_session(fallback);
        }
    }

    /// Worktrees created by Caelum, scoped to their original project. Git's full
    /// catalogue is still used for branch choices and checkout availability.
    pub fn managed_worktrees<'a>(
        &'a self,
        project: &'a str,
    ) -> impl Iterator<Item = &'a caelum_protocol::GitWorktree> {
        self.services
            .worktrees(project)
            .into_iter()
            .flat_map(|listing| &listing.worktrees)
            .filter(move |tree| {
                !tree.main
                    && self.managed_worktrees.contains(&tree.path)
                    && self.group_path(&tree.project.path) == project
            })
    }

    /// Switch to this exact checkout, including the main checkout. A project
    /// group's remembered session may belong to a different linked worktree.
    pub fn select_checkout(&mut self, path: &str) -> bool {
        if !self.projects.iter().any(|project| project.path == path)
            || self
                .services
                .worktrees(self.group_path(path))
                .is_some_and(|listing| {
                    !listing
                        .worktrees
                        .iter()
                        .any(|tree| tree.project.path == path && tree.available)
                })
        {
            return false;
        }
        let usable = |i: usize| {
            let session = &self.sessions[i];
            session.scope.project == path && !session.archived() && !session.forgotten()
        };
        let index = self
            .remembered_checkouts
            .get(path)
            .or(self.remembered.get(path))
            .copied()
            .filter(|&i| usable(i))
            .or_else(|| (0..self.sessions.len()).rev().find(|&i| usable(i)));
        if let Some(index) = index {
            self.select(index);
        } else {
            self.new_session(Scope {
                project: path.into(),
                provider: self.preferences.default_provider,
            });
        }
        true
    }

    pub fn project_is_open(&self, path: &str) -> bool {
        let root = self.group_path(path);
        self.projects
            .iter()
            .any(|p| self.group_path(&p.path) == root)
    }

    /// Close a project group in navigation, retaining sessions, drafts and work.
    pub fn close_project(&mut self, path: &str) {
        let root = self.group_path(path).to_owned();
        let Some(position) = self
            .projects
            .iter()
            .position(|p| self.group_path(&p.path) == root)
        else {
            return;
        };
        let closed: std::collections::HashSet<_> = self
            .projects
            .iter()
            .filter(|p| self.group_path(&p.path) == root)
            .map(|p| p.path.clone())
            .collect();
        let active = self
            .active_project
            .as_ref()
            .is_some_and(|p| self.group_path(p) == root);
        self.closed_projects.insert(root.clone(), self.next_request);
        self.projects.retain(|p| !closed.contains(&p.path));
        if let Some(index) = self
            .last_archived()
            .filter(|i| self.group_path(&self.sessions[*i].scope.project) == root)
        {
            self.dismiss_archive(index);
        }
        if active {
            if let Some(index) = self.selected {
                self.dismiss_side_question(index);
            }
            self.navigation += 1;
            self.active_project = None;
            self.selected = None;
            if let Some(project) = self
                .projects
                .get(position.min(self.projects.len().saturating_sub(1)))
            {
                let path = project.path.clone();
                self.select_project(&path);
            }
        }
    }

    pub(super) fn closed_since(&self, project: &str, request: u64) -> bool {
        self.closed_projects
            .get(self.group_path(project))
            .is_some_and(|closed| request <= *closed)
    }

    pub fn group_path<'a>(&'a self, path: &'a str) -> &'a str {
        let path = self
            .native_checkout(path)
            .map_or(path, |c| c.source.project.as_str());
        self.project_roots
            .get(path)
            .map(String::as_str)
            .unwrap_or(path)
    }

    pub fn refresh_project(&mut self) {
        if let Some(path) = self.active_project.clone() {
            self.task(Task::Git {
                project: path.clone(),
            });
            if crate::remote::native::checkout_id(&path).is_none() {
                self.task(Task::Worktrees {
                    project: path.clone(),
                    directory: self.preferences.worktree_directory.clone(),
                });
            }
            if let Some(branch) = self
                .services
                .checkouts
                .get(&path)
                .and_then(|c| c.git.as_ref())
                .and_then(|g| g.branch.clone())
            {
                self.task(Task::Discover {
                    project: path.clone(),
                    branch,
                });
            }
            for &provider in Provider::for_project(Some(&path)) {
                let scope = Scope {
                    project: path.clone(),
                    provider,
                };
                self.refresh_choices_for(scope.clone());

                if self
                    .pending
                    .values()
                    .any(|pending| matches!(pending, Pending::List(current) if *current == scope))
                {
                    continue;
                }
                self.notices.remove(&scope);
                self.request(Command::List(scope.clone()), Pending::List(scope));
            }
        }
    }
}
