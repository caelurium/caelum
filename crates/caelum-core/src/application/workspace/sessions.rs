use super::{update_summary, Pending, Session, Workspace};
use crate::application::{Command, Provider, Scope, Summary};

impl Workspace {
    pub fn refresh_open_sessions(&mut self) {
        let sessions: Vec<_> = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(_, session)| {
                !session.archived()
                    && !session.forgotten()
                    && (session.unbound || self.project_is_open(&session.scope.project))
            })
            .map(|(index, _)| index)
            .collect();
        for index in sessions {
            self.refresh_choices_for(self.sessions[index].scope.clone());
            self.refresh_session(index);
        }
    }

    /// Reload session history on an explicit user refresh.
    pub fn refresh_session(&mut self, index: usize) {
        self.refresh(index);
    }

    pub fn toggle_pin(&mut self, index: usize) -> bool {
        let Some(session) = self.sessions.get_mut(index) else {
            return false;
        };
        if session.forgotten() || session.archived() {
            return false;
        }
        session.pinned = !session.pinned;
        session.pinned
    }

    pub fn toggle_bookmark(&mut self, index: usize, entry_id: String) -> bool {
        let Some(session) = self.sessions.get_mut(index) else {
            return false;
        };
        if session.forgotten() || entry_id.is_empty() {
            return false;
        }
        if let Some(pos) = session
            .bookmarked_entries
            .iter()
            .position(|id| id == &entry_id)
        {
            session.bookmarked_entries.remove(pos);
            false
        } else {
            session.bookmarked_entries.push(entry_id);
            true
        }
    }

    /// Refresh both active and archived provider sessions for a picker.
    pub fn discover_sessions(&mut self, scope: Scope) {
        if !scope.provider_available() {
            return;
        }
        self.notices.remove(&scope);
        if !self
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::List(current) if current == &scope))
        {
            self.request(Command::List(scope.clone()), Pending::List(scope.clone()));
        }
        if !self
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::ListArchived(current) if current == &scope))
        {
            self.request(
                Command::ListArchived(scope.clone()),
                Pending::ListArchived(scope),
            );
        }
    }

    pub fn refresh_choices(&mut self, provider: Provider) {
        let Some(project) = self.active_project.clone() else {
            return;
        };
        self.refresh_choices_for(Scope { project, provider });
    }

    pub fn refresh_choices_for(&mut self, scope: Scope) {
        if !scope.provider_available()
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Choices(s) if s == &scope))
        {
            return;
        }
        self.notices.remove(&scope);
        self.request(
            Command::Choices(scope.clone()),
            Pending::Choices(scope.clone()),
        );
        if matches!(scope.provider, Provider::OpenCode | Provider::Antigravity) {
            self.request(Command::Commands(scope.clone()), Pending::Commands(scope));
        }
    }

    pub fn test_sound(&mut self) {
        self.request(
            Command::Sound(self.preferences.sound_volume),
            Pending::Notify,
        );
    }

    pub fn new_session(&mut self, scope: Scope) -> usize {
        self.new_session_slot(scope.for_new_session())
    }

    pub(super) fn new_session_slot(&mut self, scope: Scope) -> usize {
        let mut session = Session::new(scope.clone());
        session.record_activity(crate::application::presentation::now());
        session.options = self.project_options(&scope);
        if let Some(choices) = self.choices.get(&scope) {
            if session.options.model.is_none() {
                session.options.model = choices.default_model.clone();
            }
        }
        let index = self.sessions.len();
        self.sessions.push(session);
        self.select(index);
        index
    }

    pub fn project_draft(&mut self, scope: Scope) -> usize {
        let scope = scope.for_new_session();
        if let Some(i) = self.sessions.iter().rposition(|s| {
            s.scope == scope
                && s.remote_id.is_none()
                && s.provider_switch.threads.is_empty()
                && !s.archived()
                && !s.forgotten()
                && !s.working()
        }) {
            self.select(i);
            i
        } else {
            self.new_session(scope)
        }
    }

    pub fn acknowledge(&mut self) -> bool {
        if let Some(i) = self.selected {
            if !self.window_background
                && !self.conversation_hidden
                && self.sessions[i].loaded()
                && !self.sessions[i].loading()
                && self.sessions[i].reading < 0.0
            {
                return std::mem::take(&mut self.sessions[i].unread);
            }
        }
        false
    }

    pub fn select(&mut self, index: usize) {
        self.notifications.cancel();
        if index >= self.sessions.len() || self.sessions[index].forgotten() {
            return;
        }
        self.remote.saved.selected = None;
        if self.selected != Some(index) {
            if let Some(previous) = self.selected {
                self.dismiss_side_question(previous);
            }
            self.navigation += 1;
        }
        let Some(session) = self.sessions.get(index) else {
            return;
        };
        if session.unbound {
            self.active_project = None;
        } else {
            self.active_project = Some(session.scope.project.clone());
            self.remembered.insert(session.scope.project.clone(), index);
            self.remembered_checkouts
                .insert(session.scope.project.clone(), index);
            if let Some(root) = self.project_roots.get(&session.scope.project) {
                self.remembered.insert(root.clone(), index);
            }
        }
        self.selected = Some(index);
        if !session.loaded() || session.disconnected {
            self.refresh(index);
        }
        self.restore_previous_pages(index);
    }

    pub(super) fn creating(&self, scope: &Scope) -> bool {
        self.pending.values().any(|pending| matches!(pending, Pending::Start(index) if self.sessions[*index].scope == *scope))
    }

    pub(super) fn apply_summaries(
        &mut self,
        request: u64,
        scope: &Scope,
        summaries: Vec<Summary>,
        archived: bool,
    ) {
        for summary in summaries
            .into_iter()
            .filter(|summary| summary.key.scope == *scope)
        {
            if self
                .sessions
                .iter()
                .any(|s| s.has_previous_key(&summary.key))
            {
                continue;
            }
            if scope.provider == Provider::OpenCode
                && crate::remote::native::checkout_id(&scope.project).is_none()
            {
                if let Some(index) = self.sessions.iter().position(|session| {
                    session.scope.provider == Provider::OpenCode
                        && crate::remote::native::checkout_id(&session.scope.project).is_none()
                        && session.scope != *scope
                        && !session.forgotten()
                        && session.remote_id.as_deref() == Some(&summary.key.id)
                }) {
                    self.check_discovered_move(index, &scope.project, request);
                    continue;
                }
            }
            if let Some(session) = self
                .sessions
                .iter_mut()
                .find(|session| session.key().as_ref() == Some(&summary.key))
            {
                if session.forgotten() {
                    continue;
                }
                session.observe_archive(archived, request);
                if session.revision == 0 {
                    session.conversation.set_working(summary.working);
                    update_summary(session, summary, request);
                }
            } else {
                let mut session = Session::new(scope.clone());
                session.remote_id = Some(summary.key.id.clone());
                session.invalidate_history();
                session.conversation.set_working(summary.working);
                session.restore_archive(archived, request);
                update_summary(&mut session, summary, request);
                self.sessions.push(session);
                self.detail_queue.push(self.sessions.len() - 1);
            }
        }
        self.schedule_details();
    }

    pub(super) fn schedule_details(&mut self) {
        let count = self
            .pending
            .values()
            .filter(|p| matches!(p, Pending::Details(_)))
            .count();
        for _ in count..2 {
            let Some(index) = self.detail_queue.pop() else {
                break;
            };
            if let Some(key) = self.sessions[index]
                .key()
                .filter(|k| k.scope.provider_available())
            {
                self.request(Command::Details(key), Pending::Details(index));
            }
        }
    }
}
