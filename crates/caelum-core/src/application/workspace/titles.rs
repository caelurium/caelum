use super::{Pending, Session, Workspace};
use crate::application::{Command, Provider};
use crate::remote::{native::checkout_id, Chat};

const TURN_INTERVAL: u8 = 3;
const TITLE_CHARS: usize = 200;
const CONTINUED_SESSION: &str = "Continued session";

fn headline(text: &str, limit: usize) -> &str {
    let text = text.trim_start();
    let end = text.char_indices().enumerate().find_map(|(n, (i, c))| {
        (n == limit || c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')).then_some(i)
    });
    text[..end.unwrap_or(text.len())].trim_end()
}

pub(super) fn valid_title(title: &str) -> bool {
    !title.is_empty() && headline(title, TITLE_CHARS) == title
}

impl Session {
    /// Accept a label for the toolbar, feed and notifications. A handoff preview
    /// is not a rename and must not advance the live title's request barrier.
    pub(super) fn suggest_title(&mut self, text: &str) -> bool {
        let title = headline(text, TITLE_CHARS);
        if self.title_state.manual || title.is_empty() {
            return false;
        }
        if checkout_id(&self.scope.project).is_some() && Chat::is_handoff_preview(title) {
            if self.title == "New session" {
                self.title = CONTINUED_SESSION.into();
            }
            return false;
        }
        title.clone_into(&mut self.title);
        true
    }

    pub(super) fn first_prompt_title(&mut self, prompt: &str) {
        if self.native_handoff.is_none()
            || matches!(
                self.title.as_str(),
                "New session" | "Home draft" | CONTINUED_SESSION
            )
        {
            self.suggest_title(headline(prompt, 60));
        }
    }

    pub(super) fn restore_title(&mut self, title: String, manual: bool) {
        if manual && valid_title(&title) {
            self.title = title;
        } else {
            self.suggest_title(&title);
        }
        self.title_state.manual = manual;
    }
}

#[derive(Default)]
pub(super) struct TitleState {
    pub(super) manual: bool,
    /// Successful executions since the last request, capped to coalesce work.
    pub(super) turns: u8,
    /// Snapshots requested before a live rename must not restore an old title.
    pub(super) barrier: u64,
}

impl TitleState {
    pub(super) fn completed_turn(&mut self) {
        if !self.manual {
            self.turns = self.turns.saturating_add(1).min(TURN_INTERVAL);
        }
    }
}

impl Workspace {
    pub(super) fn refresh_title(&mut self, index: usize) {
        let session = &mut self.sessions[index];
        if session.scope.provider != Provider::OpenCode
            || session.title_state.manual
            || session.title_state.turns < TURN_INTERVAL
            || session.archived
            || session.archiving
            || session.forgotten
            || session.deleting
            || session.disconnected
        {
            return;
        }
        let Some(key) = session.key() else {
            return;
        };
        if self
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::RegenerateTitle(_, owner) | Pending::ConversationControl(_,owner,super::controls::Operation::Rename(_)) if *owner == key))
        {
            return;
        }
        // Failures wait for another interval rather than repeatedly charging
        // for retries. Turns completed in flight accrue toward one follow-up.
        session.title_state.turns = 0;
        self.request(
            Command::RegenerateTitle(key.clone()),
            Pending::RegenerateTitle(index, key),
        );
    }
}
