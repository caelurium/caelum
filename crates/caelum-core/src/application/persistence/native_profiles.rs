use super::{Snapshot, Store};
use crate::remote::native::{project_key, NativeTarget};

impl Store {
    /// Import the former per-VM window's drafts into regular stable session slots.
    /// The main snapshot and migration receipt are replaced atomically. A profile
    /// still open in an older desktop is left alone and retried on the next launch.
    pub(crate) fn load_workspace(&self) -> Result<Option<Snapshot>, String> {
        let Some(mut state) = self.load()? else {
            return Ok(None);
        };
        let mut changed = false;
        for (id, c) in &mut state.remote.checkouts {
            let imported = state.remote.imported_profiles.contains(id);
            if imported && c.defaults.is_some() {
                continue;
            }
            let Some(machine) = c.machine() else {
                continue;
            };
            let target = NativeTarget {
                machine: machine.key.clone(),
                repository: c.source.repository.clone(),
                checkout_root: c.root.clone().unwrap_or_default(),
            };
            let legacy = Store(target.store_path(&self.0));
            if !legacy.0.is_file() {
                continue;
            }
            let Ok(_lock) = legacy.lock() else {
                continue;
            };
            let Ok(Some(mut old)) = legacy.load() else {
                continue;
            };
            let Some(root) = old
                .projects
                .first()
                .map(|p| p.path.as_str())
                .or_else(|| old.sessions.first().map(|s| s.scope.project.as_str()))
                .or(c.root.as_deref())
                .map(str::to_owned)
            else {
                continue;
            };
            let project = project_key(id);
            if old.sessions.iter().any(|s| s.scope.project != root)
                || old.projects.iter().any(|p| p.path != root)
            {
                continue;
            }
            if c.defaults.is_none() {
                c.defaults = Some(crate::remote::ProviderDefaults {
                    provider: old.preferences.default_provider,
                    options: old.preferences.providers,
                    favorites: old.preferences.favorite_models,
                });
                changed = true;
            }
            if imported {
                continue;
            }
            let start = state.sessions.len();
            for mut s in old.sessions {
                s.scope.project.clone_from(&project);
                state.sessions.push(s);
            }
            if let Some(i) = old
                .selected
                .and_then(|n| start.checked_add(n))
                .filter(|i| *i < state.sessions.len())
            {
                state.remembered_checkouts.insert(project.clone(), i);
            }
            if let Some(files) = old.preferences.files.remove(&root) {
                state
                    .preferences
                    .files
                    .entry(project.clone())
                    .or_insert(files);
            }
            if old.home.project.as_deref() == Some(&root)
                && (!old.home.text.is_empty()
                    || !old.home.attachments.is_empty()
                    || old.home.options != Default::default()
                    || old.home.provider.is_some())
            {
                let mut draft = super::migration::saved(
                    crate::application::Scope {
                        project,
                        provider: old
                            .home
                            .provider
                            .unwrap_or(old.preferences.default_provider),
                    },
                    None,
                );
                draft.draft = old.home.text;
                draft.options = old.home.options;
                draft.attachments = old.home.attachments;
                state.sessions.push(draft);
            }
            state.remote.imported_profiles.insert(id.clone());
            changed = true;
        }
        if changed {
            self.save(&state)?;
        }
        Ok(Some(state))
    }
}
