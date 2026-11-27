//! Compare the persisted projection by reference. Public editor/preferences fields
//! can change without a reducer event, so an event-only dirty flag is insufficient.
use super::super::{Session, Workspace};
use crate::application::{persistence::FileWorkspace, Preferences, SavedSession, Snapshot};
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
mod tests;

impl Workspace {
    pub(super) fn matches_snapshot(&self, snapshot: &Snapshot) -> bool {
        let Snapshot {
            version,
            remote,
            home,
            projects,
            sessions,
            selected,
            preferences,
            remembered,
            remembered_checkouts,
            managed_worktrees,
            project_roots,
        } = snapshot;
        let retained = || self.sessions.iter().filter(|s| !s.forgotten());
        let saved_indices: Option<Vec<Option<usize>>> = (self.sessions.len() != sessions.len())
            .then(|| {
                let mut next = 0usize;
                self.sessions
                    .iter()
                    .map(|s| {
                        if s.forgotten() {
                            None
                        } else {
                            let idx = next;
                            next += 1;
                            Some(idx)
                        }
                    })
                    .collect()
            });
        let saved_index = |index: usize| match &saved_indices {
            Some(map) => map.get(index).copied().flatten(),
            None => self
                .sessions
                .get(index)
                .filter(|s| !s.forgotten())
                .map(|_| index),
        };
        let remembered_matches =
            |current: &HashMap<String, usize>, saved: &BTreeMap<String, usize>| {
                let mut count = 0usize;
                for (key, &index) in current {
                    if let Some(mapped) = saved_index(index) {
                        if saved.get(key) != Some(&mapped) {
                            return false;
                        }
                        count += 1;
                    }
                }
                count == saved.len()
            };
        *version == 1
            && self.remote.saved == *remote
            && self.home.draft == *home
            && self.projects == *projects
            && retained().count() == sessions.len()
            && retained()
                .zip(sessions)
                .all(|(s, saved)| s.matches_saved(saved))
            && self.selected.and_then(saved_index) == *selected
            && self.managed_worktrees == *managed_worktrees
            && self.project_roots == *project_roots
            && remembered_matches(&self.remembered, remembered)
            && remembered_matches(&self.remembered_checkouts, remembered_checkouts)
            && self.matches_preferences(preferences)
    }

    fn matches_preferences(&self, saved: &Preferences) -> bool {
        let Preferences {
            files,
            theme,
            theme_family,
            accent_color,
            home_background,
            peaceful_mode,
            session_scope,
            default_provider,
            worktree_directory,
            worktree_expanded,
            trusted_worktree_setup,
            providers,
            favorite_models,
            notifications,
            notification_mode,
            sound_volume,
            background_shells,
            input_needed,
        } = &self.preferences;
        if *theme != saved.theme
            || *theme_family != saved.theme_family
            || *accent_color != saved.accent_color
            || *home_background != saved.home_background
            || *peaceful_mode != saved.peaceful_mode
            || *session_scope != saved.session_scope
            || *default_provider != saved.default_provider
            || *worktree_directory != saved.worktree_directory
            || *worktree_expanded != saved.worktree_expanded
            || *trusted_worktree_setup != saved.trusted_worktree_setup
            || *providers != saved.providers
            || *favorite_models != saved.favorite_models
            || *notifications != saved.notifications
            || *notification_mode != saved.notification_mode
            || *sound_volume != saved.sound_volume
            || *background_shells != saved.background_shells
            || *input_needed != saved.input_needed
        {
            return false;
        }

        if files.keys().any(|path| !saved.files.contains_key(path))
            || self
                .services
                .checkouts
                .keys()
                .any(|path| !path.is_empty() && !saved.files.contains_key(path))
        {
            return false;
        }
        let empty = FileWorkspace::default();
        saved.files.iter().all(|(path, saved)| {
            let checkout = self
                .services
                .checkouts
                .get(path)
                .filter(|_| !path.is_empty());
            let current = files.get(path);
            if current.is_none() && checkout.is_none() {
                return false;
            }
            let FileWorkspace {
                scroll,
                tabs,
                selected,
                expanded,
                line,
            } = current.unwrap_or(&empty);
            *scroll == saved.scroll
                && *expanded == saved.expanded
                && *line == saved.line
                && checkout.map_or(tabs, |c| &c.tabs) == &saved.tabs
                && checkout.map_or(selected, |c| &c.selected_file) == &saved.selected
        })
    }
}

impl Session {
    fn matches_saved(&self, saved: &SavedSession) -> bool {
        let SavedSession {
            previous_threads,
            needs_context,
            native_handoff,
            vm_link,
            last_activity,
            scope,
            remote_id,
            title,
            title_refresh_turns,
            manual_title,
            options,
            draft,
            attachments,
            failed_prompt,
            pending_submission,
            queue,
            inbox,
            archived,
            reading,
            unread,
            origin,
            pinned,
            unbound,
            bookmarked_entries,
            checkpoints,
        } = saved;
        self.vm_link == *vm_link
            && (std::sync::Arc::ptr_eq(&self.provider_switch.threads, previous_threads)
                || self.provider_switch.threads == *previous_threads)
            && self.provider_switch.needs_context == *needs_context
            && self.native_handoff == *native_handoff
            && self.last_activity == *last_activity
            && self.scope == *scope
            && self.remote_id == *remote_id
            && self.title == *title
            && self.title_state.turns() == *title_refresh_turns
            && self.title_state.manual() == *manual_title
            && self.options == *options
            && self.draft == *draft
            && self.attachments == *attachments
            && self.failed_prompt == *failed_prompt
            && self.pending_prompt() == pending_submission.as_ref()
            && self.queue == *queue
            && self.inbox.matches_saved(inbox)
            && self.archived() == *archived
            && self.reading == *reading
            && self.unread == *unread
            && self.origin == *origin
            && self.pinned == *pinned
            && self.unbound == *unbound
            && self.bookmarked_entries == *bookmarked_entries
            && self.checkpoints == *checkpoints
    }
}
