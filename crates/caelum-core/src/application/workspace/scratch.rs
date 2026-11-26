//! Unbound ("General") scratch sessions that can be promoted to a project at any time.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{Pending, Session, Workspace};
use crate::application::{Provider, Scope, Store};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(1);

fn scratch_root() -> PathBuf {
    let state = Store::default_path();
    let base = state
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    base.join("scratch")
}

/// Shared scratch directory used for Home's model/provider choice scope when no project is picked.
pub fn home_scratch_path() -> String {
    let dir = scratch_root().join("general");
    let _ = fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

/// Allocates a fresh isolated scratch directory for a new unbound General session.
pub fn new_session_dir() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = scratch_root().join(format!("general-{now}-{seq}"));
    let _ = fs::create_dir_all(&dir);
    dir.to_string_lossy().into_owned()
}

impl Workspace {
    /// Creates and selects a new unbound General session backed by an isolated scratch directory.
    pub fn new_general_session(&mut self, provider: Provider) -> usize {
        let scope = Scope {
            project: new_session_dir(),
            provider,
        };
        let mut session = Session::new(scope.clone());
        session.unbound = true;
        session.options = self.provider_options(&scope).cloned().unwrap_or_default();
        if session.options.model.is_none() {
            session.options.model = self
                .choices
                .get(&scope)
                .and_then(|c| c.default_model.clone());
        }
        let index = self.sessions.len();
        self.sessions.push(session);
        self.select(index);
        self.refresh_choices_for(scope);
        index
    }

    /// Returns whether an unbound session can currently be attached to a project.
    pub fn can_attach_to_project(&self, index: usize) -> bool {
        let Some(s) = self.sessions.get(index) else {
            return false;
        };
        if !s.unbound {
            return false;
        }
        let pending_backend = self.pending.values().any(|p| match p {
            Pending::Details(i)
            | Pending::Shells(i, _)
            | Pending::Inbox(i, _)
            | Pending::VerifyOpenCodeMove(i)
            | Pending::RegenerateTitle(i)
            | Pending::Fork(super::fork::Request::Create { index: i, .. }) => *i == index,
            _ => false,
        });
        s.can_send()
            && !s.commanding()
            && !s.control_busy()
            && !s.generating
            && !s.deleting()
            && s.queue.is_empty()
            && s.inbox.items.is_empty()
            && s.shells.is_empty()
            && !s
                .conversation
                .entries
                .iter()
                .any(|e| e.name == "subagent" && !e.complete())
            && !self.needs_input(s)
            && !pending_backend
    }

    /// Promotes an unbound General session into `project`.
    /// Draft-only sessions rebind in place; active threads retain their visible transcript and
    /// hand off conversation context into a new provider session scoped to `project`.
    pub fn attach_to_project(&mut self, index: usize, project: String) -> bool {
        if !self.can_attach_to_project(index) || project.trim().is_empty() {
            return false;
        }
        let provider = self.sessions[index].scope.provider;
        let scope = Scope {
            provider,
            project: project.clone(),
        };
        let s = &mut self.sessions[index];
        if let Some(key) = s.key() {
            s.retain_thread(key);
            s.reset_backend();
            if let Some(last) = s.provider_switch.threads.last() {
                s.provider_switch.needs_context = !last.context.messages.is_empty();
                s.title_state = super::titles::TitleState::Manual;
            }
            s.remote_id = None;
            s.options_dirty = true;
            s.details = None;
            s.history = Default::default();
            s.inbox = Default::default();
            s.seen_events.clear();
            s.controls = Default::default();
        }
        s.scope = scope.clone();
        s.unbound = false;
        s.error = None;
        s.revision += 1;
        self.active_project = Some(project.clone());
        self.remembered.insert(project.clone(), index);
        self.remembered_checkouts.insert(project, index);
        self.refresh_choices_for(scope);
        true
    }
}
