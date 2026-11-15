use super::{update_summary, Pending, Workspace};
use crate::application::{Command, Provider, Summary};
use caelum_protocol::SessionHistoryPage;

impl Workspace {
    pub(super) fn complete_resume(
        &mut self,
        request: u64,
        index: usize,
        revision: u64,
        summary: Summary,
        history: Option<SessionHistoryPage>,
    ) {
        let session = &mut self.sessions[index];
        if session.key().as_ref() != Some(&summary.key) {
            session.loading = false;
            session.error = Some("Provider returned a different session".into());
            return;
        }
        if session.revision == revision {
            session.conversation.working = summary.working;
        }
        update_summary(session, summary, request);
        let key = session.key().expect("resumed session");
        let needs_history = history.is_none();
        if let Some(page) = history {
            self.complete_history(index, revision, false, page);
        }
        if key.scope.provider == Provider::OpenCode {
            self.request(
                Command::Shells(key.clone()),
                Pending::Shells(index, self.sessions[index].shell_revision),
            );
            self.refresh_inbox(index);
        }
        if needs_history {
            self.request(
                Command::History(key, None),
                Pending::History(index, revision, false),
            );
        }
    }

    pub(super) fn complete_history(
        &mut self,
        index: usize,
        revision: u64,
        older: bool,
        page: SessionHistoryPage,
    ) {
        let session = &mut self.sessions[index];
        session
            .conversation
            .history(session.scope.provider, &page.data, older || !session.loaded);
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
        if older || !session.loaded {
            session.next_cursor = page.next_cursor;
        }
        session.loaded = true;
        session.loading = false;
        session.disconnected = false;
    }
}
