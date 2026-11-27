use super::Live;
use crate::panels::Screen;
use caelum_core::application::{Event, Provider, Session, Workspace, presentation::TurnPreview};
use rune::Tx;

pub(crate) struct StreamSnapshot {
    screen: Screen,
    overlay: bool,
    selected: Option<usize>,
    sessions: Vec<(usize, u64, bool)>,
    remote: Option<(String, usize, usize, bool)>,
}

/// Text deltas can change these visible session properties even while their
/// transcript is hidden (for example, the first delta can mark a session busy).
struct Summary {
    title: String,
    working: bool,
    retrying: bool,
    unread: bool,
    failure: Option<u64>,
    last_activity: Option<u64>,
    preview: Preview,
}

enum Preview {
    Hidden,
    Child(String),
    Overview(TurnPreview),
}

impl Summary {
    fn new(session: &Session, preview: Preview) -> Self {
        Self {
            title: session.title.clone(),
            working: session.working(),
            retrying: session.conversation.retrying(),
            unread: session.unread,
            failure: session.conversation.failure.notice().map(|n| n.generation),
            last_activity: session.last_activity,
            preview,
        }
    }

    fn matches(&self, app: &Workspace, session: &Session) -> bool {
        self.title == session.title
            && self.working == session.working()
            && self.retrying == session.conversation.retrying()
            && self.unread == session.unread
            && self.failure == session.conversation.failure.notice().map(|n| n.generation)
            && self.last_activity == session.last_activity
            && match &self.preview {
                Preview::Hidden => true,
                Preview::Child(previous) => *previous == peek_text(session),
                Preview::Overview(previous) => *previous == app.turn_preview(session),
            }
    }
}

fn peek_text(session: &Session) -> String {
    caelum_core::application::presentation::latest_message(&session.conversation.entries)
        .map_or_else(String::new, |(_, text)| text)
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
            if let Event::Runner(ev) = event {
                let updated = self.app.runner.apply(ev);
                changed |= updated && self.panels.overlay() == Some(crate::panels::Screen::Runner);
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
                    .is_none_or(|s| !summary.matches(&self.app, s))
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
        if (self.app.selected == Some(index)
            && self.panels.screen != Screen::Home
            && self.panels.screen != Screen::Overview)
            || owners.next().is_some()
        {
            return None;
        }
        let peek = provider == Provider::OpenCode
            && self.panels.screen == crate::panels::Screen::Conversation
            && self.panels.overlay().is_none()
            && !self.expanded.is_disjoint(&self.visible_entries)
            && self.app.selected.is_some_and(|i| {
                let parent = &self.app.sessions[i];
                parent.scope == session.scope
                    && parent
                        .conversation
                        .entries
                        .iter()
                        .rev()
                        .filter(|e| e.name == "subagent")
                        .any(|e| {
                            let row = (i, e.id.clone());
                            self.visible_entries.contains(&row)
                                && self.expanded.contains(&row)
                                && caelum_core::application::subagents::Subagent::from_entry(e)
                                    .is_some_and(|t| t.session_id.as_deref() == Some(id))
                        })
            })
            && crate::questions::active(self, index).is_none();
        let preview = if self.overview.shows_turn(index) {
            Preview::Overview(self.app.turn_preview(session))
        } else if peek {
            Preview::Child(peek_text(session))
        } else {
            Preview::Hidden
        };
        Some((index, Summary::new(session, preview)))
    }

    pub(crate) fn stream_snapshot(&self) -> StreamSnapshot {
        let overlay = self.panels.overlay().is_some();
        let mut sessions = Vec::with_capacity(2);
        if self.panels.screen == Screen::Conversation && !overlay {
            let mut push = |index: usize| {
                if let Some(session) = self.app.sessions.get(index)
                    && !sessions.iter().any(|(seen, _, _)| *seen == index)
                {
                    sessions.push((index, session.conversation.activity, session.working()));
                }
            };
            if let Some(index) = self.app.selected {
                push(index);
            }
            if self.split.is_visible(self.window_width.get())
                && let Some(crate::split::SecondaryPane::Session(index)) =
                    self.split.visible_secondary()
            {
                push(*index);
            }
        }
        let remote = (self.panels.screen == Screen::Remote && !overlay)
            .then(|| {
                let id = self.app.remote.saved.selected.as_deref()?;
                let checkout = self.app.remote.saved.checkouts.get(id)?;
                Some((
                    id.to_owned(),
                    checkout.thread.lines.len(),
                    checkout.thread.lines.last().map_or(0, |l| l.text.len()),
                    checkout.thread.lines.iter().any(|l| l.partial),
                ))
            })
            .flatten();
        StreamSnapshot {
            screen: self.panels.screen,
            overlay,
            selected: self.app.selected,
            sessions,
            remote,
        }
    }

    pub(crate) fn stream_progressed(&self, before: &StreamSnapshot, history_loaded: bool) -> bool {
        if history_loaded
            || before.overlay
            || self.panels.overlay().is_some()
            || before.screen != self.panels.screen
            || self.last_screen != self.panels.screen
        {
            return false;
        }
        match self.panels.screen {
            Screen::Conversation => {
                before.selected == self.app.selected
                    && self.last_selected == self.app.selected
                    && before
                        .sessions
                        .iter()
                        .any(|&(index, activity, was_working)| {
                            self.app.sessions.get(index).is_some_and(|session| {
                                session.conversation.activity != activity
                                    && (was_working || session.working())
                            })
                        })
            }
            Screen::Remote => before.remote.as_ref().is_some_and(
                |(id, prev_lines, prev_last_len, was_partial)| {
                    self.app.remote.saved.selected.as_deref() == Some(id.as_str())
                        && self.app.remote.saved.checkouts.get(id).is_some_and(|c| {
                            let lines = c.thread.lines.len();
                            let last_len = c.thread.lines.last().map_or(0, |l| l.text.len());
                            let is_partial = c.thread.lines.iter().any(|l| l.partial);
                            (lines != *prev_lines || last_len != *prev_last_len)
                                && (*was_partial || is_partial)
                        })
                },
            ),
            _ => false,
        }
    }

    pub(crate) fn finish_stream_poll(&mut self, before: &StreamSnapshot, tx: &mut Tx) -> bool {
        let mut history_loaded = false;
        self.history_lengths.resize(self.app.sessions.len(), 0);
        for (index, session) in self.app.sessions.iter().enumerate() {
            let length = session.conversation.entries.len();
            let was_working = before
                .sessions
                .iter()
                .any(|&(seen, _, working)| seen == index && working);
            if length != self.history_lengths[index]
                && !session.working()
                && !was_working
                && self.app.selected == Some(index)
            {
                tx.instant();
                history_loaded = true;
            }
            self.history_lengths[index] = length;
        }
        let animate = self.stream_progressed(before, history_loaded);
        if animate {
            tx.animated();
        }
        animate
    }
}
