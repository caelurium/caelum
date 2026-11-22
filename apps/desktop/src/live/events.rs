use super::Live;
use caelum_core::application::{Event, Provider, Session};

/// Text deltas can change these visible session properties even while their
/// transcript is hidden (for example, the first delta can mark a session busy).
struct Summary {
    title: String,
    working: bool,
    retrying: bool,
    unread: bool,
    failure: Option<u64>,
    last_activity: Option<u64>,
}

impl Summary {
    fn new(session: &Session) -> Self {
        Self {
            title: session.title.clone(),
            working: session.working(),
            retrying: session.conversation.retrying(),
            unread: session.unread,
            failure: session.conversation.failure.notice().map(|n| n.generation),
            last_activity: session.last_activity,
        }
    }

    fn matches(&self, session: &Session) -> bool {
        self.title == session.title
            && self.working == session.working()
            && self.retrying == session.conversation.retrying()
            && self.unread == session.unread
            && self.failure == session.conversation.failure.notice().map(|n| n.generation)
            && self.last_activity == session.last_activity
    }
}

impl Live {
    /// Apply every event immediately; only the view invalidation can be skipped.
    /// Unknown event kinds and lifecycle/permission/completion events repaint.
    pub(crate) fn apply_events(&mut self, events: impl IntoIterator<Item = Event>) -> bool {
        let mut changed = false;
        for event in events {
            if let Event::UpdateProgress(id, progress) = event {
                let updated = self.app.services.updater.progress(id, progress);
                changed |= updated
                    && self.panels.overlay() == Some(crate::panels::Screen::Settings)
                    && self.panels.settings_page == crate::settings::Page::About;
                continue;
            }
            let trace = caelum_core::diagnostics::event(&event).filter(|mark| {
                self.app.selected.is_some_and(|i| {
                    mark.owner() != 0
                        && mark.owner()
                            == caelum_core::diagnostics::session_owner(&self.app.sessions[i])
                }) && self.panels.screen == crate::panels::Screen::Conversation
                    && self.panels.overlay().is_none()
            });
            let before = (!changed).then(|| self.background_delta(&event)).flatten();
            self.apply(event);
            self.latency.track(trace, self.app.selected, false);
            changed |= before.is_none_or(|(index, summary)| {
                self.app
                    .sessions
                    .get(index)
                    .is_none_or(|s| !summary.matches(s))
            });
        }
        changed
    }

    fn background_delta(&self, event: &Event) -> Option<(usize, Summary)> {
        let (provider, project, data, id) = match event {
            Event::Codex(e)
                if matches!(
                    e.method.as_str(),
                    "item/agentMessage/delta"
                        | "item/reasoning/summaryTextDelta"
                        | "item/reasoning/textDelta"
                ) =>
            {
                (
                    Provider::Codex,
                    &e.project_path,
                    &e.params,
                    e.params["threadId"]
                        .as_str()
                        .or(e.params["thread"]["id"].as_str())?,
                )
            }
            Event::OpenCode(e)
                if matches!(
                    e.event_type.as_str(),
                    "session.text.delta" | "session.reasoning.delta"
                ) =>
            {
                (
                    Provider::OpenCode,
                    &e.project_path,
                    &e.data,
                    e.data["sessionID"]
                        .as_str()
                        .or(e.data["session"]["id"].as_str())?,
                )
            }
            Event::Antigravity(e)
                if matches!(
                    e.method.as_str(),
                    "agent_message_chunk" | "agent_thought_chunk"
                ) =>
            {
                (
                    Provider::Antigravity,
                    &e.project_path,
                    &e.params,
                    e.session_id.as_deref().or(e.params["sessionId"].as_str())?,
                )
            }
            _ => return None,
        };

        if data["message"]["type"].is_string() {
            return None;
        }
        let mut owners = self.app.sessions.iter().enumerate().filter(|(_, s)| {
            s.scope.provider == provider
                && s.scope.project == *project
                && s.remote_id.as_deref() == Some(id)
        });
        let (index, session) = owners.next()?;
        if (self.app.selected == Some(index) && self.panels.screen != crate::panels::Screen::Home)
            || owners.next().is_some()
        {
            return None;
        }
        Some((index, Summary::new(session)))
    }
}
