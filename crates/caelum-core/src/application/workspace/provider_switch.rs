use super::{Pending, Session, Workspace};
use crate::{
    application::{
        persistence::PreviousThread, presentation, Command, Conversation, Entry, EntryKind,
        Provider, Reply, Scope, SessionKey,
    },
    remote::{Chat, Message, RecentChat},
};
use std::sync::Arc;

#[derive(Default)]
pub(super) struct State {
    pub(super) threads: Arc<Vec<PreviousThread>>,
    pub(super) needs_context: bool,
    pages: Vec<Page>,
}

enum Page {
    Unread,
    Ready(String),
    Reading(u64, Option<String>),
    Done,
}

impl Session {
    pub(super) fn restore_threads(
        &mut self,
        threads: Arc<Vec<PreviousThread>>,
        needs_context: bool,
    ) {
        self.provider_switch.pages = threads.iter().map(|_| Page::Unread).collect();
        self.provider_switch.threads = threads;
        self.provider_switch.needs_context = needs_context;
        if let Some(last) = self.provider_switch.threads.last() {
            self.conversation.context_prefix(&last.context);
        }
    }

    pub(super) fn has_previous_key(&self, key: &SessionKey) -> bool {
        self.provider_switch.threads.iter().any(|t| &t.key == key)
    }

    pub(super) fn reset_backend(&mut self) {
        let entries = std::mem::take(&mut self.conversation.entries);
        self.conversation = Conversation::default();
        self.conversation.entries = entries;
        if let Some(last) = self.provider_switch.threads.last() {
            self.conversation.context_prefix(&last.context);
        }
    }

    pub fn previous_scope(&self, entry: &Entry) -> Option<&Scope> {
        Some(
            &self
                .provider_switch
                .threads
                .get(entry.previous_thread?)?
                .key
                .scope,
        )
    }

    /// Stable across switches, including when two backends reuse a message ID.
    pub fn transcript_id<'a>(&self, entry: &'a Entry) -> std::borrow::Cow<'a, str> {
        match entry.previous_thread {
            Some(0) => entry
                .id
                .strip_prefix("previous-0-")
                .unwrap_or(&entry.id)
                .into(),
            Some(_) => entry.id.as_str().into(),
            None if self.provider_switch.threads.is_empty() => entry.id.as_str().into(),
            None => format!(
                "previous-{}-{}",
                self.provider_switch.threads.len(),
                entry.id
            )
            .into(),
        }
    }

    pub(super) fn previous_cursor(&self) -> Option<&str> {
        self.provider_switch
            .pages
            .iter()
            .rev()
            .find_map(|page| match page {
                Page::Ready(cursor) => Some(cursor.as_str()),
                Page::Unread => Some(""),
                _ => None,
            })
    }

    pub(super) fn retain_thread(&mut self, key: SessionKey) {
        let segment = self.provider_switch.threads.len();
        let cursor = self.active_cursor().map(str::to_owned);
        let context = context(self);
        for row in self
            .conversation
            .entries
            .iter_mut()
            .rev()
            .take_while(|r| r.previous_thread.is_none())
        {
            freeze(row, segment);
        }
        Arc::make_mut(&mut self.provider_switch.threads).push(PreviousThread { key, context });
        self.provider_switch
            .pages
            .push(cursor.map_or(Page::Done, Page::Ready));
    }
}

fn freeze(row: &mut Entry, segment: usize) {
    row.previous_thread = Some(segment);
    row.id = format!("previous-{segment}-{}", row.id);
}

fn context(s: &Session) -> Chat {
    let previous = s.provider_switch.threads.last().map(|t| &t.context);
    let current = s
        .conversation
        .entries
        .iter()
        .rev()
        .take_while(|r| r.previous_thread.is_none())
        .filter_map(|r| {
            let make = match r.kind() {
                EntryKind::User => Message::User,
                EntryKind::Assistant => Message::Assistant,
                _ => return None,
            };
            let text = presentation::entry_text(r);
            (!text.trim().is_empty()).then(|| make(text))
        });
    let mut chat = RecentChat::default();
    for message in current.chain(
        previous
            .into_iter()
            .flat_map(|c| c.messages.iter().rev().cloned()),
    ) {
        if !chat.push(message) {
            break;
        }
    }
    chat.finish(previous.is_some_and(|c| c.clipped) || s.active_cursor().is_some())
}

impl Workspace {
    pub fn available_providers(&self, project: Option<&str>) -> Vec<Provider> {
        let project_providers = Provider::for_project(project);
        if project
            .and_then(crate::remote::native::checkout_id)
            .is_some()
        {
            return project_providers.to_vec();
        }
        match &self.services.installed_providers {
            Some(installed) => project_providers
                .iter()
                .copied()
                .filter(|provider| installed.contains(provider))
                .collect(),
            None => project_providers.to_vec(),
        }
    }

    pub fn selectable_providers(&self, project: Option<&str>) -> Vec<Provider> {
        let available = self.available_providers(project);
        if available.is_empty() {
            Provider::for_project(project).to_vec()
        } else {
            available
        }
    }

    pub fn set_installed_providers(&mut self, providers: Vec<Provider>) {
        self.services.installed_providers = Some(providers);
    }

    pub fn can_switch_provider(&self, index: usize) -> bool {
        let Some(s) = self.sessions.get(index) else {
            return false;
        };
        let pending_backend = self.pending.values().any(|p| match p {
            Pending::Details(i)
            | Pending::Shells(i, _)
            | Pending::Inbox(i, _)
            | Pending::VerifyOpenCodeMove(i)
            | Pending::RegenerateTitle(i)
            | Pending::Fork(super::fork::Request::Create { index: i, .. }) => *i == index,
            _ => false,
        });
        self.selected == Some(index)
            && self
                .available_providers(Some(&s.scope.project))
                .iter()
                .any(|&provider| provider != s.scope.provider)
            && s.can_send()
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

    pub fn switch_provider(&mut self, index: usize, provider: Provider) -> bool {
        if !self.can_switch_provider(index)
            || self.sessions[index].scope.provider == provider
            || !self
                .available_providers(Some(&self.sessions[index].scope.project))
                .contains(&provider)
        {
            return false;
        }
        let scope = Scope {
            provider,
            project: self.sessions[index].scope.project.clone(),
        };
        let mut options = self.provider_options(&scope).cloned().unwrap_or_default();
        if options.model.is_none() {
            options.model = self
                .choices
                .get(&scope)
                .and_then(|c| c.default_model.clone());
        }
        let s = &mut self.sessions[index];
        if let Some(key) = s.key() {
            s.retain_thread(key);
        }
        s.reset_backend();
        if let Some(last) = s.provider_switch.threads.last() {
            s.provider_switch.needs_context = !last.context.messages.is_empty();
            s.title_state = super::titles::TitleState::Manual;
        }
        s.remote_id = None;
        s.scope = scope.clone();
        s.options = options;
        s.options_dirty = true;
        s.details = None;
        s.history = Default::default();
        s.inbox = Default::default();
        s.seen_events.clear();
        s.controls = Default::default();
        s.error = None;
        s.revision += 1;
        self.refresh_choices_for(scope);
        true
    }

    pub(super) fn restore_previous_pages(&mut self, index: usize) {
        let pages = &self.sessions[index].provider_switch.pages;
        if !pages.iter().any(|p| matches!(p, Page::Reading(..))) {
            if let Some(segment) = pages.iter().rposition(|p| matches!(p, Page::Unread)) {
                self.request_previous_page(index, segment);
            }
        }
    }

    pub(super) fn read_previous_page(&mut self, index: usize) {
        if let Some(segment) = self.sessions[index]
            .provider_switch
            .pages
            .iter()
            .rposition(|p| matches!(p, Page::Ready(_) | Page::Unread))
        {
            self.request_previous_page(index, segment);
        }
    }

    fn request_previous_page(&mut self, index: usize, segment: usize) {
        let state = &self.sessions[index].provider_switch;
        let cursor = match &state.pages[segment] {
            Page::Ready(cursor) => Some(cursor.clone()),
            Page::Unread => None,
            _ => return,
        };
        let key = state.threads[segment].key.clone();
        let request = self.request(
            Command::History(key, cursor.clone()),
            Pending::PreviousHistory(index, segment),
        );
        self.sessions[index].provider_switch.pages[segment] = Page::Reading(request, cursor);
    }

    pub(super) fn complete_previous_history(
        &mut self,
        index: usize,
        segment: usize,
        request: u64,
        result: Result<Reply, String>,
    ) {
        let s = &mut self.sessions[index];
        let Some(Page::Reading(owner, cursor)) = s.provider_switch.pages.get(segment) else {
            return;
        };
        if *owner != request || s.forgotten() {
            return;
        }
        let page = match result {
            Ok(Reply::History(page)) => page,
            other => {
                s.error = Some(
                    other
                        .err()
                        .unwrap_or_else(|| "Could not read earlier messages.".into()),
                );
                s.provider_switch.pages[segment] = cursor.clone().map_or(Page::Unread, Page::Ready);
                return;
            }
        };
        let mut conversation = Conversation::default();
        if segment > 0 {
            conversation.context_prefix(&s.provider_switch.threads[segment - 1].context);
        }
        conversation.history(
            s.provider_switch.threads[segment].key.scope.provider,
            &page.data,
            false,
        );
        for row in &mut conversation.entries {
            freeze(row, segment);
        }
        let rows = &mut s.conversation.entries;
        let start = rows.partition_point(|r| r.previous_thread.is_some_and(|i| i < segment));
        let existing: std::collections::HashSet<_> = rows[start..]
            .iter()
            .take_while(|r| r.previous_thread == Some(segment))
            .map(|r| r.id.as_str())
            .collect();
        conversation
            .entries
            .retain(|r| !existing.contains(r.id.as_str()));
        rows.splice(start..start, conversation.entries);
        s.provider_switch.pages[segment] = page
            .next_cursor
            .filter(|next| !next.is_empty() && Some(next) != cursor.as_ref())
            .map_or(Page::Done, Page::Ready);
        if self.selected == Some(index) {
            self.restore_previous_pages(index);
        }
    }
}
