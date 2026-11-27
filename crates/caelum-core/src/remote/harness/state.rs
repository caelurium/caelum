use super::{Continuation, Run, RunStatus, Thread, Turn, VmProvider};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Saved", into = "Saved")]
pub(super) enum State {
    Draft(Option<Continuation>, DraftTurn),
    AwaitingNative(Continuation),
    Managed {
        conversation: String,
        cursor: Option<String>,
        continued: Option<Continuation>,
        turn: Turn,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DraftTurn {
    Idle,
    Submitting,
}

impl Default for State {
    fn default() -> Self {
        Self::Draft(None, DraftTurn::Idle)
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    awaiting_native: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    continued: Option<Continuation>,
    conversation: Option<String>,
    cursor: Option<String>,
    turn: Turn,
}

impl From<Saved> for State {
    fn from(saved: Saved) -> Self {
        let Saved {
            awaiting_native,
            continued,
            conversation,
            cursor,
            turn,
        } = saved;
        let conversation = conversation.or_else(|| turn.run().map(|r| r.conversation.clone()));
        match conversation {
            Some(conversation) => {
                let mismatch = turn.run().is_some_and(|r| r.conversation != conversation);
                let (cursor, turn) = if mismatch {
                    (None, Turn::Submitting)
                } else {
                    (cursor, turn)
                };
                Self::Managed {
                    conversation,
                    cursor,
                    continued,
                    turn,
                }
            }
            None if turn == Turn::Submitting => Self::Draft(continued, DraftTurn::Submitting),
            None => match continued {
                Some(continued) if awaiting_native => Self::AwaitingNative(continued),
                continued => Self::Draft(continued, DraftTurn::Idle),
            },
        }
    }
}

impl From<State> for Saved {
    fn from(state: State) -> Self {
        match state {
            State::Draft(continued, turn) => Self {
                awaiting_native: false,
                continued,
                conversation: None,
                cursor: None,
                turn: match turn {
                    DraftTurn::Idle => Turn::Idle,
                    DraftTurn::Submitting => Turn::Submitting,
                },
            },
            State::AwaitingNative(continued) => Self {
                awaiting_native: true,
                continued: Some(continued),
                conversation: None,
                cursor: None,
                turn: Turn::Idle,
            },
            State::Managed {
                conversation,
                cursor,
                continued,
                turn,
            } => Self {
                awaiting_native: false,
                continued,
                conversation: Some(conversation),
                cursor,
                turn,
            },
        }
    }
}

impl Thread {
    pub fn with_continuation(provider: VmProvider, continued: Continuation, draft: String) -> Self {
        Self {
            provider,
            draft,
            state: State::Draft(Some(continued), DraftTurn::Idle),
            ..Self::default()
        }
    }

    pub fn awaiting_native(&self) -> bool {
        matches!(self.state, State::AwaitingNative(_))
    }

    pub fn continuation(&self) -> Option<&Continuation> {
        match &self.state {
            State::Draft(continued, _) | State::Managed { continued, .. } => continued.as_ref(),
            State::AwaitingNative(continued) => Some(continued),
        }
    }

    pub fn conversation(&self) -> Option<&str> {
        match &self.state {
            State::Managed { conversation, .. } => Some(conversation),
            _ => None,
        }
    }

    pub fn turn(&self) -> &Turn {
        match &self.state {
            State::Managed { turn, .. } => turn,
            State::Draft(_, DraftTurn::Submitting) => &Turn::Submitting,
            State::Draft(_, DraftTurn::Idle) | State::AwaitingNative(_) => &Turn::Idle,
        }
    }

    pub(crate) fn cursor(&self) -> Option<&str> {
        match &self.state {
            State::Managed { cursor, .. } => cursor.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn await_native(&mut self) {
        if let State::Draft(continued, DraftTurn::Idle) = &mut self.state {
            if let Some(continued) = continued.take() {
                self.state = State::AwaitingNative(continued);
            }
        }
    }

    pub(crate) fn take_native_continuation(&mut self) -> Option<Continuation> {
        match std::mem::take(&mut self.state) {
            State::AwaitingNative(continued) => Some(continued),
            state => {
                self.state = state;
                None
            }
        }
    }

    pub(crate) fn begin_submission(&mut self) {
        match &mut self.state {
            State::Draft(_, turn @ DraftTurn::Idle) => *turn = DraftTurn::Submitting,
            State::Managed {
                turn: turn @ Turn::Idle,
                ..
            } => *turn = Turn::Submitting,
            _ => {}
        }
    }

    pub(crate) fn finish_turn(&mut self) {
        match &mut self.state {
            State::Draft(_, turn) => *turn = DraftTurn::Idle,
            State::Managed { turn, .. } => *turn = Turn::Idle,
            State::AwaitingNative(_) => {}
        }
    }

    pub(crate) fn accept_run(&mut self, run: Run) -> bool {
        if self.turn() != &Turn::Submitting
            || self.provider != run.provider
            || self.conversation().is_some_and(|id| id != run.conversation)
        {
            return false;
        }
        match &mut self.state {
            State::Managed { turn, .. } => *turn = Turn::Following(run),
            State::Draft(continued, DraftTurn::Submitting) => {
                self.state = State::Managed {
                    conversation: run.conversation.clone(),
                    cursor: None,
                    continued: continued.take(),
                    turn: Turn::Following(run),
                };
            }
            _ => return false,
        }
        true
    }

    pub(crate) fn stop_run(&mut self, expected: &Run) {
        if let State::Managed { turn, .. } = &mut self.state {
            if turn.run() == Some(expected) {
                *turn = Turn::Stopping(expected.clone());
            }
        }
    }

    pub(crate) fn record_page(&mut self, next: Option<String>, status: &RunStatus) {
        if let State::Managed { cursor, turn, .. } = &mut self.state {
            if let Some(run) = turn.run() {
                *cursor = next;
                if matches!(turn, Turn::Stopping(_)) {
                    return;
                }
                *turn = match status {
                    RunStatus::Queued => Turn::Queued(run.clone()),
                    RunStatus::Running => Turn::Following(run.clone()),
                    RunStatus::Finishing => Turn::Finishing(run.clone()),
                    RunStatus::Ended(_) => return,
                };
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn with_saved_turn(
        provider: VmProvider,
        conversation: Option<String>,
        turn: Turn,
    ) -> Self {
        Self {
            provider,
            state: Saved {
                awaiting_native: false,
                continued: None,
                conversation,
                cursor: None,
                turn,
            }
            .into(),
            ..Self::default()
        }
    }
}
