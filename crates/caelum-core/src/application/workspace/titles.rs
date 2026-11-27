use super::{Pending, Workspace};
use crate::application::{Command, Provider, SessionKey};

const TURN_INTERVAL: u8 = 3;

#[derive(Default)]
pub(super) struct Automatic {
    turns: u8,
    barrier: u64,
    generated: bool,
    update: Option<Update>,
}

struct Update {
    request: u64,
    owner: SessionKey,
}

pub(super) enum TitleState {
    Automatic(Automatic),
    Manual,
}

impl Default for TitleState {
    fn default() -> Self {
        Self::Automatic(Automatic::default())
    }
}

impl TitleState {
    pub(super) fn restored(manual: bool, turns: u8) -> Self {
        if manual {
            Self::Manual
        } else {
            Self::Automatic(Automatic {
                turns: turns.min(TURN_INTERVAL),
                ..Automatic::default()
            })
        }
    }

    pub(super) fn manual(&self) -> bool {
        matches!(self, Self::Manual)
    }

    pub(super) fn turns(&self) -> u8 {
        match self {
            Self::Automatic(state) => state.turns,
            Self::Manual => 0,
        }
    }

    pub(super) fn allows_snapshot(&self, request: u64) -> bool {
        matches!(self, Self::Automatic(state) if request > state.barrier)
    }

    pub(super) fn note_live_rename(&mut self, request: u64) {
        if let Self::Automatic(state) = self {
            state.barrier = request;
            state.generated = true;
        }
    }

    fn due(&self, provider: Provider) -> bool {
        match self {
            Self::Automatic(state) => {
                let threshold = if provider == Provider::Codex && !state.generated {
                    1
                } else {
                    TURN_INTERVAL
                };
                state.turns >= threshold
            }
            Self::Manual => false,
        }
    }

    pub(super) fn updating(&self, owner: &SessionKey) -> bool {
        matches!(self, Self::Automatic(Automatic { update: Some(update), .. }) if &update.owner == owner)
    }

    pub(super) fn completed_turn(&mut self) {
        if let Self::Automatic(state) = self {
            state.turns = state.turns.saturating_add(1).min(TURN_INTERVAL);
        }
    }
}

impl Workspace {
    pub(super) fn refresh_title(&mut self, index: usize) {
        let session = &mut self.sessions[index];
        if !session.title_state.due(session.scope.provider)
            || session.archived()
            || session.archiving()
            || session.forgotten()
            || session.deleting()
            || session.disconnected
        {
            return;
        }
        let Some(key) = session.key() else {
            return;
        };
        if session.title_state.updating(&key) || self
            .pending
            .values()
            .any(|pending| matches!(pending, Pending::ConversationControl(_,owner,super::controls::Operation::Rename(_)) if *owner == key))
        {
            return;
        }

        let request = self.request(
            Command::RegenerateTitle(key.clone()),
            Pending::RegenerateTitle(index),
        );
        if let TitleState::Automatic(state) = &mut self.sessions[index].title_state {
            state.turns = 0;
            state.generated = true;
            state.update = Some(Update {
                request,
                owner: key,
            });
        }
    }

    pub(super) fn finish_title_update(&mut self, index: usize, request: u64) {
        let session = &mut self.sessions[index];
        let key = session.key();
        let TitleState::Automatic(state) = &mut session.title_state else {
            return;
        };
        if state
            .update
            .as_ref()
            .is_some_and(|update| update.request == request)
        {
            let update = state.update.take().unwrap();
            if key.as_ref() == Some(&update.owner) {
                self.refresh_title(index);
            }
        }
    }
}
