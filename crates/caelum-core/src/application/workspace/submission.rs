use super::{Pending, Session, Workspace};
use crate::application::{Command, Provider, QueuedPrompt, SessionKey, Summary};
use caelum_protocol::{ApprovalDecision, ApprovalPrompt, QuestionPrompt};

#[derive(Default)]
pub(super) enum Submission {
    #[default]
    Idle,
    Creating {
        request: u64,
        attempt: Attempt,
    },
    Admitting {
        request: u64,
        key: SessionKey,
        revision: u64,
        attempt: Attempt,
    },
    Stopping {
        request: u64,
    },
}

pub(super) struct Attempt {
    prompt: QueuedPrompt,
    origin: Origin,
    intent: Intent,
}

#[derive(Clone, Copy)]
pub(super) enum Origin {
    Direct,
    Queue,
}

enum Intent {
    Continue,
    Stop,
}

pub(super) struct NativeCommand {
    request: u64,
    prompt: String,
}

impl Session {
    pub fn submitting(&self) -> bool {
        self.pending_prompt().is_some()
    }

    pub fn stopping(&self) -> bool {
        matches!(
            self.submission,
            Submission::Stopping { .. }
                | Submission::Creating {
                    attempt: Attempt {
                        intent: Intent::Stop,
                        ..
                    },
                    ..
                }
                | Submission::Admitting {
                    attempt: Attempt {
                        intent: Intent::Stop,
                        ..
                    },
                    ..
                }
        )
    }

    pub fn commanding(&self) -> bool {
        self.native_command.is_some()
    }

    pub(super) fn pending_prompt(&self) -> Option<&QueuedPrompt> {
        match &self.submission {
            Submission::Creating { attempt, .. } | Submission::Admitting { attempt, .. } => {
                Some(&attempt.prompt)
            }
            _ => None,
        }
    }

    fn recover_prompt(&mut self, attempt: Attempt) {
        match attempt.origin {
            Origin::Direct => self.failed_prompt = Some(attempt.prompt.text),
            Origin::Queue => self.queue.insert(0, attempt.prompt),
        }
    }
}

impl Workspace {
    /// Ordinary messages wait behind active work, history/inbox reads and earlier prompts.
    pub fn will_queue(&self, index: usize) -> bool {
        let session = &self.sessions[index];
        session.working()
            || session.loading()
            || !session.queue.is_empty()
            || !session.inbox.items.is_empty()
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Inbox(i, _) if *i == index))
    }

    pub fn send(&mut self, index: usize, prompt: String) -> bool {
        if prompt.trim().is_empty() && self.sessions[index].attachments.is_empty() {
            return false;
        }
        let queued = self.will_queue(index);
        let session = &mut self.sessions[index];
        if !session.can_submit() {
            return false;
        }
        let native_command = prompt
            .trim()
            .strip_prefix('/')
            .filter(|_| !prompt.trim().starts_with("//"));
        if let Some(command) = native_command {
            let name = command.split_whitespace().next().unwrap_or_default();
            if !self
                .provider_commands
                .get(&session.scope)
                .is_some_and(|commands| commands.iter().any(|c| c.name == name))
            {
                session.error =
                    Some(
                        if self.pending.values().any(
                            |p| matches!(p, Pending::Commands(scope) if scope == &session.scope),
                        ) {
                            "Commands are loading. Try again when discovery finishes.".into()
                        } else {
                            format!("Unknown command /{name}")
                        },
                    );
                return false;
            }
        }
        let prompt = crate::application::composer::with_context(&prompt, &session.attachments);
        if let Some(key) = session.key().filter(|_| native_command.is_some()) {
            session.error = None;
            session.attachments.clear();
            session.draft.clear();
            session.options_dirty = false;
            let command = Command::Send(key, prompt.clone(), session.options.clone());
            let request = self.request(command, Pending::Control(index));
            self.sessions[index].native_command = Some(NativeCommand { request, prompt });
            return true;
        }
        if session.working()
            && session.scope.provider == Provider::OpenCode
            && !session.options_dirty
            && session.queue.is_empty()
            && !session.queue_paused
        {
            if let Some(key) = session.key() {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                let id = format!("msg_caelum_{stamp}_{}", self.next_request);
                session.attachments.clear();
                session.draft.clear();
                let request = self.request(
                    Command::Enqueue(key, id.clone(), prompt.clone()),
                    Pending::Enqueue(index, id.clone()),
                );
                self.sessions[index].inbox.enqueue(id, prompt, request);
                return true;
            }
        }
        if queued {
            session.queue.push(QueuedPrompt {
                text: prompt,
                options: session.options.clone(),
            });
            session.attachments.clear();
            session.draft.clear();
            return true;
        }
        let accepted = self.send_raw(index, prompt);
        if accepted {
            self.sessions[index].attachments.clear();
            self.sessions[index].draft.clear();
        }
        accepted
    }

    pub fn needs_input(&self, session: &Session) -> bool {
        self.approvals.iter().any(|p| {
            p.project_path == session.scope.project
                && p.provider == session.scope.provider.as_str()
                && (p.session_id.is_none() || p.session_id == session.remote_id)
        }) || self.questions.iter().any(|p| {
            p.project_path == session.scope.project
                && p.provider == session.scope.provider.as_str()
                && Some(&p.session_id) == session.remote_id.as_ref()
        })
    }

    pub fn review_blocker(&self, index: usize, project: &str) -> Option<&'static str> {
        let session = self.sessions.get(index)?;
        if session.scope.project != project {
            return Some("Choose a session in this checkout");
        }
        if session.working() {
            return Some("Wait for the current turn to finish");
        }
        if self.needs_input(session) {
            return Some("Answer the session’s pending request first");
        }
        if !session.can_send() || session.remote_id.is_none() {
            return Some("Connect an active session to address feedback");
        }
        if !session.queue.is_empty() || !session.inbox.items.is_empty() {
            return Some("Finish queued messages first");
        }
        None
    }

    /// Send review feedback to its captured destination, retaining the independent composer draft.
    pub fn address_review(&mut self, index: usize, project: &str, url: &str) -> bool {
        if index >= self.sessions.len() || self.review_blocker(index, project).is_some() {
            return false;
        }
        let Some(prompt) = self
            .services
            .reviews
            .get(&(project.into(), url.into()))
            .and_then(|r| r.address_prompt())
        else {
            return false;
        };
        self.send_raw(index, prompt)
    }

    fn send_raw(&mut self, index: usize, text: String) -> bool {
        let prompt = QueuedPrompt {
            text,
            options: self.sessions[index].options.clone(),
        };
        self.submit_prompt(index, prompt, Origin::Direct)
    }

    pub(super) fn submit_prompt(
        &mut self,
        index: usize,
        prompt: QueuedPrompt,
        origin: Origin,
    ) -> bool {
        let session = &mut self.sessions[index];
        if prompt.text.trim().is_empty() || !session.can_send() {
            return false;
        }
        session.record_activity(crate::application::presentation::now());
        session.error = None;
        session.options_dirty = false;
        session.revision += 1;
        session.conversation.begin_turn(
            prompt.options.agent.clone(),
            crate::application::presentation::now(),
        );
        session.conversation.optimistic(
            format!("local-{}", self.next_request + 1),
            prompt.text.clone(),
        );
        let attempt = Attempt {
            prompt,
            origin,
            intent: Intent::Continue,
        };
        if let Some(key) = session.key() {
            self.admit_prompt(index, key, attempt);
        } else {
            let text = &attempt.prompt.text;
            session.title = text
                .lines()
                .next()
                .unwrap_or(text)
                .chars()
                .take(60)
                .collect();
            let command = Command::Start(session.scope.clone(), attempt.prompt.options.clone());
            let request = self.request(command, Pending::Start(index));
            self.sessions[index].submission = Submission::Creating { request, attempt };
        }
        true
    }

    fn admit_prompt(&mut self, index: usize, key: SessionKey, attempt: Attempt) {
        let revision = self.sessions[index].revision;
        let request = self.request(
            Command::Send(
                key.clone(),
                attempt.prompt.text.clone(),
                attempt.prompt.options.clone(),
            ),
            Pending::Send(index),
        );
        self.sessions[index].submission = Submission::Admitting {
            request,
            key,
            revision,
            attempt,
        };
    }

    pub fn cancel(&mut self, index: usize) {
        let session = &mut self.sessions[index];
        if !session.working() || session.stopping() {
            return;
        }
        session.queue_paused = true;
        let previous_revision = session.revision;
        session.revision += 1;
        match &mut session.submission {
            Submission::Creating { attempt, .. } => {
                attempt.intent = Intent::Stop;
            }
            Submission::Admitting {
                attempt, revision, ..
            } => {
                attempt.intent = Intent::Stop;
                if *revision == previous_revision {
                    *revision = session.revision;
                }
            }
            Submission::Idle => {
                if let Some(key) = session.key() {
                    self.stop_provider(index, key);
                }
            }
            Submission::Stopping { .. } => {}
        }
    }

    fn stop_provider(&mut self, index: usize, key: SessionKey) {
        let request = self.request(Command::Cancel(key), Pending::Cancel(index));
        self.sessions[index].submission = Submission::Stopping { request };
    }

    pub(super) fn complete_creation(
        &mut self,
        index: usize,
        id: u64,
        result: Result<Summary, String>,
    ) {
        let session = &mut self.sessions[index];
        if !matches!(session.submission, Submission::Creating { request, .. } if request == id) {
            return;
        }
        let Submission::Creating { attempt, .. } = std::mem::take(&mut session.submission) else {
            unreachable!();
        };
        let result = result.and_then(|summary| {
            (session.scope == summary.key.scope)
                .then_some(summary.key)
                .ok_or_else(|| "Provider created a session in a different project".to_string())
        });
        match result {
            Ok(key) => {
                session.remote_id = Some(key.id.clone());
                match attempt.intent {
                    Intent::Continue => self.admit_prompt(index, key, attempt),
                    Intent::Stop => {
                        session.conversation.settle();
                        session.conversation.remove_optimistic();
                        session.recover_prompt(attempt);
                    }
                }
            }
            Err(error) => {
                session.queue_paused = true;
                session.conversation.settle();
                session.conversation.remove_optimistic();
                session.recover_prompt(attempt);
                session.error = Some(error);
            }
        }
    }

    pub(super) fn complete_admission(&mut self, index: usize, id: u64, result: Result<(), String>) {
        let session = &mut self.sessions[index];
        if !matches!(session.submission, Submission::Admitting { request, .. } if request == id) {
            return;
        }
        let Submission::Admitting {
            key,
            revision,
            attempt,
            ..
        } = std::mem::take(&mut session.submission)
        else {
            unreachable!();
        };
        match result {
            Ok(()) if matches!(attempt.intent, Intent::Stop) => self.stop_provider(index, key),
            Ok(()) => {
                if !session.working() {
                    self.refresh(index);
                }
            }
            Err(error) => {
                session.queue_paused = true;
                if session.revision == revision {
                    session.conversation.working = false;
                }
                if session
                    .conversation
                    .entries
                    .iter()
                    .any(|entry| entry.optimistic)
                {
                    session.conversation.remove_optimistic();
                    session.recover_prompt(attempt);
                }
                session.error = Some(error);
            }
        }
    }

    pub(super) fn complete_stop(&mut self, index: usize, id: u64, result: Result<(), String>) {
        let session = &mut self.sessions[index];
        if !matches!(session.submission, Submission::Stopping { request } if request == id) {
            return;
        }
        session.submission = Submission::Idle;
        match result {
            Ok(()) => {
                session.revision += 1;
                session.conversation.settle();
                self.refresh(index);
            }
            Err(error) => session.error = Some(error),
        }
    }

    pub(super) fn complete_native_command(
        &mut self,
        index: usize,
        id: u64,
        result: Result<(), String>,
    ) {
        let session = &mut self.sessions[index];
        if session
            .native_command
            .as_ref()
            .is_none_or(|command| command.request != id)
        {
            return;
        }
        let command = session.native_command.take().unwrap();
        if let Err(error) = result {
            session.error = Some(error);
            session.failed_prompt = Some(command.prompt);
        }
    }

    pub fn approve(&mut self, prompt: ApprovalPrompt, decision: ApprovalDecision) {
        let owner = Pending::Request {
            provider: prompt.provider.clone(),
            project: prompt.project_path.clone(),
            session: prompt.session_id.clone(),
        };
        self.request(Command::Approve(prompt, decision), owner);
    }

    pub fn answer(
        &mut self,
        prompt: QuestionPrompt,
        answer: Option<crate::questions::Answer>,
    ) -> Option<u64> {
        if let Some(answer) = &answer {
            let checked = crate::application::forms::validate(&prompt, answer);
            if !checked.errors.is_empty() {
                return None;
            }
        }
        let owner = Pending::Request {
            provider: prompt.provider.clone(),
            project: prompt.project_path.clone(),
            session: Some(prompt.session_id.clone()),
        };
        Some(self.request(Command::Answer(prompt, answer), owner))
    }
}
