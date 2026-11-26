use super::{Pending, Session, Workspace};
use crate::application::{composer::with_context, Command, Provider};
use tokio::sync::watch;

#[derive(Default)]
pub(super) enum State {
    #[default]
    Editing,
    ContextError(String),
    Answering(Attempt),
    Finished(Exchange, Result<(), String>),
}

pub(super) struct Exchange {
    question: String,
    answer: String,
}

pub(super) struct Attempt {
    request: u64,
    exchange: Exchange,
    execution: Execution,
}

enum Execution {
    Endpoint,
    Streaming { _cancel: watch::Sender<()> },
}

impl Session {
    pub fn generating_side_answer(&self) -> bool {
        matches!(self.side_question, State::Answering(_))
    }

    pub fn side_exchange(&self) -> Option<(&str, &str)> {
        let exchange = match &self.side_question {
            State::Answering(attempt) => &attempt.exchange,
            State::Finished(exchange, _) => exchange,
            State::Editing | State::ContextError(_) => return None,
        };
        Some((&exchange.question, &exchange.answer))
    }

    pub fn side_error(&self) -> Option<&str> {
        match &self.side_question {
            State::ContextError(error) | State::Finished(_, Err(error)) => Some(error),
            _ => None,
        }
    }

    pub fn edit_side_question(&mut self) {
        if !self.generating_side_answer() {
            self.side_question = State::Editing;
        }
    }

    pub(super) fn set_side_context_error(&mut self, error: Option<String>) {
        if matches!(self.side_question, State::Editing | State::ContextError(_)) {
            self.side_question = error.map_or(State::Editing, State::ContextError);
        }
    }

    pub(super) fn update_side_answer(&mut self, request: u64, text: String) {
        if let State::Answering(attempt) = &mut self.side_question {
            if attempt.request == request
                && matches!(attempt.execution, Execution::Streaming { .. })
            {
                attempt.exchange.answer = text;
            }
        }
    }

    pub(super) fn finish_side_question(&mut self, request: u64, result: Result<String, String>) {
        self.side_question = match std::mem::take(&mut self.side_question) {
            State::Answering(mut attempt) if attempt.request == request => {
                let outcome = result.map(|answer| attempt.exchange.answer = answer);
                State::Finished(attempt.exchange, outcome)
            }
            phase => phase,
        };
    }

    pub(in crate::application::workspace) fn interrupt_side_question(&mut self, error: &str) {
        if let State::Answering(attempt) = &self.side_question {
            self.finish_side_question(attempt.request, Err(error.into()));
        }
    }
}

impl Workspace {
    pub fn dismiss_side_question(&mut self, index: usize) {
        self.cancel_mentions(index);
        let session = &mut self.sessions[index];
        session.side_question = State::Editing;
        session.side_draft.clear();
        session.side_attachments.clear();
    }

    pub fn ask(&mut self, index: usize, question: String) -> bool {
        let request = self.next_request_id();
        let session = &mut self.sessions[index];
        if !session.scope.provider_available()
            || session.forgotten()
            || session.archived()
            || session.disconnected
            || session.generating_side_answer()
            || session.rewinding()
            || session.attaching > 0
            || question.trim().is_empty()
        {
            return false;
        }
        let Some(key) = session.key() else {
            session.error = Some("Start a session before asking a side question".into());
            return false;
        };
        session.side_draft.clone_from(&question);
        let context = with_context(&question, &session.side_attachments);
        let exchange = Exchange {
            question,
            answer: String::new(),
        };
        let (command, execution) = if key.scope.provider == Provider::OpenCode {
            (Command::Generate(key, context), Execution::Endpoint)
        } else {
            let text = match crate::side_questions::prompt(&session.conversation, &context) {
                Ok(text) => text,
                Err(error) => {
                    session.side_question = State::Finished(exchange, Err(error));
                    return false;
                }
            };
            let (cancel, until) = watch::channel(());
            let turn = crate::side_questions::Turn {
                generation: request,
                text,
                model: session.options.model.clone(),
                until,
            };
            (
                Command::SideTurn(key, turn),
                Execution::Streaming { _cancel: cancel },
            )
        };
        session.side_question = State::Answering(Attempt {
            request,
            exchange,
            execution,
        });
        self.queue_request(request, command, Pending::Generate(index));
        true
    }
}
