use super::Workspace;
use crate::{
    application::{Options, Provider, Scope, Task},
    project_config::ProjectConfig,
};

#[cfg(test)]
#[path = "project_config_tests.rs"]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorktreeSetup {
    pub root: String,
    pub worktree: String,
    pub commands: Vec<String>,
    pub fingerprint: String,
}

impl Workspace {
    /// Returns the loaded `.caelum/config.toml` for `project`, falling back to its root
    /// repository checkout when inspecting a worktree before its own config loads.
    pub fn project_config(&self, project: &str) -> Option<&ProjectConfig> {
        self.services
            .checkouts
            .get(project)
            .and_then(|c| c.project_config.as_ref())
            .or_else(|| {
                let root = self.group_path(project);
                (root != project)
                    .then(|| self.services.checkouts.get(root))
                    .flatten()
                    .and_then(|c| c.project_config.as_ref())
            })
    }

    pub fn project_config_error(&self, project: &str) -> Option<&str> {
        self.services
            .checkouts
            .get(project)
            .and_then(|c| c.project_config_error.as_deref())
    }

    pub fn refresh_project_config(&mut self, project: &str) {
        if crate::remote::native::checkout_id(project).is_some() {
            return;
        }
        self.task(Task::ProjectConfig {
            project: project.to_owned(),
        });
    }

    /// Preferred provider for a new session in `project`, honoring `.caelum/config.toml`
    /// when that provider is available for the checkout.
    pub fn project_default_provider(&self, project: &str) -> Provider {
        let preferred = self
            .project_config(project)
            .and_then(|c| c.defaults.provider)
            .unwrap_or(self.preferences.default_provider);
        let available = self.available_providers(Some(project));
        if available.is_empty() || available.contains(&preferred) {
            preferred
        } else {
            self.preferences.default_provider
        }
    }

    /// Starting model/effort/agent options for a new session in `scope`, overlaying
    /// `.caelum/config.toml` `[defaults]` over user provider defaults when applicable.
    pub fn project_options(&self, scope: &Scope) -> Options {
        let mut options = self.provider_options(scope).cloned().unwrap_or_default();
        if let Some(cfg) = self.project_config(&scope.project) {
            let matches_provider = cfg
                .defaults
                .provider
                .is_none_or(|provider| provider == scope.provider);
            if matches_provider {
                if let Some(model) = &cfg.defaults.model {
                    options.model = Some(model.clone());
                }
                if let Some(effort) = &cfg.defaults.effort {
                    options.effort = Some(effort.clone());
                }
                if let Some(agent) = &cfg.defaults.agent {
                    options.agent = Some(agent.clone());
                }
            }
        }
        options
    }

    pub fn is_worktree_setup_trusted(&self, project: &str) -> bool {
        let root = self.group_path(project);
        let Some(fingerprint) = self
            .project_config(project)
            .and_then(ProjectConfig::setup_fingerprint)
        else {
            return false;
        };
        self.preferences
            .trusted_worktree_setup
            .get(root)
            .is_some_and(|saved| saved == &fingerprint)
    }

    pub fn pending_worktree_setup(&self, project: &str) -> Option<&PendingWorktreeSetup> {
        self.pending_worktree_setups.get(project)
    }

    pub fn run_pending_worktree_setup(&mut self, worktree: &str, remember_trust: bool) -> bool {
        let Some(pending) = self.pending_worktree_setups.remove(worktree) else {
            return false;
        };
        if remember_trust {
            self.preferences
                .trusted_worktree_setup
                .insert(pending.root, pending.fingerprint);
        }
        let combined = pending.commands.join(" && ");
        self.run_checkout_command(pending.worktree, combined)
            .is_some()
    }

    pub fn dismiss_pending_worktree_setup(&mut self, worktree: &str) {
        self.pending_worktree_setups.remove(worktree);
    }

    pub(super) fn handle_created_worktree_setup(&mut self, root: &str, worktree_path: &str) {
        let Some(cfg) = self.project_config(root).cloned() else {
            return;
        };
        let Some(fingerprint) = cfg.setup_fingerprint() else {
            return;
        };
        if self
            .preferences
            .trusted_worktree_setup
            .get(root)
            .is_some_and(|saved| saved == &fingerprint)
        {
            if let Some(command) = cfg.combined_setup_command() {
                self.run_checkout_command(worktree_path.to_owned(), command);
            }
        } else {
            self.pending_worktree_setups.insert(
                worktree_path.to_owned(),
                PendingWorktreeSetup {
                    root: root.to_owned(),
                    worktree: worktree_path.to_owned(),
                    commands: cfg.worktree.setup,
                    fingerprint,
                },
            );
        }
    }
}
