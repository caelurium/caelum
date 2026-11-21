use super::{update_summary, Pending, Session, Workspace};
use crate::application::{Command, Provider, Summary};
use caelum_protocol::SessionHistoryPage;

pub(super) enum State {
    Idle(Availability),
    Resuming(Availability, Read),
    Fetching(Availability, Read),
    Paging(String, Read),
}

pub(super) enum Availability {
    Unloaded,
    Loaded(Option<String>),
}

pub(super) struct Read {
    request: u64,
    revision: u64,
}

impl Default for State {
    fn default() -> Self {
        Self::Idle(Availability::Loaded(None))
    }
}

impl State {
    fn cancel(&mut self) {
        let available = match std::mem::take(self) {
            Self::Idle(available) | Self::Resuming(available, _) | Self::Fetching(available, _) => {
                available
            }
            Self::Paging(cursor, _) => Availability::Loaded(Some(cursor)),
        };
        *self = Self::Idle(available);
    }

    fn request(&self) -> Option<u64> {
        match self {
            Self::Idle(_) => None,
            Self::Resuming(_, read) | Self::Fetching(_, read) | Self::Paging(_, read) => {
                Some(read.request)
            }
        }
    }
}

impl Session {
    pub fn loading(&self) -> bool {
        self.history.request().is_some()
    }

    pub fn loaded(&self) -> bool {
        !matches!(
            self.history,
            State::Idle(Availability::Unloaded)
                | State::Resuming(Availability::Unloaded, _)
                | State::Fetching(Availability::Unloaded, _)
        )
    }

    pub fn next_cursor(&self) -> Option<&str> {
        match &self.history {
            State::Idle(Availability::Loaded(cursor))
            | State::Resuming(Availability::Loaded(cursor), _)
            | State::Fetching(Availability::Loaded(cursor), _) => cursor.as_deref(),
            State::Paging(cursor, _) => Some(cursor),
            _ => None,
        }
    }

    pub fn invalidate_history(&mut self) {
        self.history = State::Idle(Availability::Unloaded);
    }

    pub(super) fn cancel_history_read(&mut self) {
        self.history.cancel();
    }

    pub(super) fn finish_resume(&mut self, request: u64) -> bool {
        if matches!(&self.history, State::Resuming(_, read) if read.request == request) {
            self.cancel_history_read();
            true
        } else {
            false
        }
    }
}

impl Workspace {
    pub fn refresh(&mut self, index: usize) {
        let session = &mut self.sessions[index];
        if session.loading()
            || session.submitting
            || session.relocation.pending()
            || session.rewinding()
            || session.forgotten
        {
            return;
        }
        let Some(key) = session.key() else {
            return;
        };
        session.error = None;
        let revision = session.revision;
        let request = self.request(Command::Resume(key), Pending::Resume(index));
        let State::Idle(available) = std::mem::take(&mut self.sessions[index].history) else {
            unreachable!();
        };
        self.sessions[index].history = State::Resuming(available, Read { request, revision });
    }

    pub fn older(&mut self, index: usize) {
        let session = &self.sessions[index];
        if session.rewinding() || session.relocation.pending() || session.forgotten {
            return;
        }
        let (Some(key), State::Idle(Availability::Loaded(Some(cursor)))) =
            (session.key(), &session.history)
        else {
            return;
        };
        let cursor = cursor.clone();
        let revision = session.revision;
        let request = self.request(
            Command::History(key, Some(cursor.clone())),
            Pending::History(index),
        );
        self.sessions[index].history = State::Paging(cursor, Read { request, revision });
    }

    pub(super) fn fail_history(&mut self, index: usize, request: u64, error: String) {
        let session = &mut self.sessions[index];
        if session.history.request() == Some(request) {
            session.cancel_history_read();
            session.error = Some(error);
        }
    }

    pub(super) fn complete_resume(
        &mut self,
        request: u64,
        index: usize,
        summary: Summary,
        history: Option<SessionHistoryPage>,
    ) {
        let session = &mut self.sessions[index];
        let State::Resuming(
            _,
            Read {
                request: owner,
                revision,
            },
        ) = &session.history
        else {
            return;
        };
        if *owner != request {
            return;
        }
        let revision = *revision;
        if session.key().as_ref() != Some(&summary.key) {
            self.fail_history(
                index,
                request,
                "Provider returned a different session".into(),
            );
            return;
        }
        if session.revision == revision {
            session.conversation.working = summary.working;
        }
        update_summary(session, summary, request);
        let key = session.key().expect("resumed session");
        let needs_page = history.is_none();
        if let Some(page) = history {
            self.complete_history(index, request, page);
        }
        if key.scope.provider == Provider::OpenCode {
            self.request(
                Command::Shells(key.clone()),
                Pending::Shells(index, self.sessions[index].shell_revision),
            );
            self.refresh_inbox(index);
        }
        if needs_page {
            let request = self.request(Command::History(key, None), Pending::History(index));
            let State::Resuming(available, _) = std::mem::take(&mut self.sessions[index].history)
            else {
                unreachable!();
            };
            self.sessions[index].history = State::Fetching(available, Read { request, revision });
        }
    }

    pub(super) fn complete_history(
        &mut self,
        index: usize,
        request: u64,
        page: SessionHistoryPage,
    ) {
        let session = &mut self.sessions[index];
        let (read, older) = match &session.history {
            State::Resuming(_, read) | State::Fetching(_, read) => (read, false),
            State::Paging(_, read) => (read, true),
            State::Idle(_) => return,
        };
        if read.request != request {
            return;
        }
        let revision = read.revision;
        let loaded = session.loaded();
        session
            .conversation
            .history(session.scope.provider, &page.data, older || !loaded);
        if session.scope.provider == Provider::OpenCode {
            for message in &page.data {
                if message["type"] == "user" {
                    if let Some(id) = message["id"].as_str() {
                        session.inbox.settle(id);
                    }
                }
            }
        }
        if !older && session.revision == revision {
            if session.scope.provider == Provider::OpenCode {
                session
                    .conversation
                    .failure
                    .history(&page.data, page.is_working.unwrap_or(session.working()));
                if session.conversation.failure.notice().is_some() {
                    session.queue_paused = true;
                }
            }
            if let Some(working) = page.is_working {
                session.conversation.working = working;
            }
            if page.model.is_some() && !session.options_dirty {
                session.options.model = page.model;
                session.options.effort = page.effort;
            }
            if page.agent.is_some() && !session.options_dirty {
                session.options.agent = page.agent;
            }
        }
        let cursor = if older || !loaded {
            page.next_cursor
        } else {
            session.next_cursor().map(str::to_owned)
        };
        session.history = State::Idle(Availability::Loaded(cursor));
        session.disconnected = false;
    }
}
