//! Reconnect provider-moved sessions without changing their stable local slots.
use super::{Pending, Workspace};
use crate::application::{Command, OpenCodeMove, Provider, Reply, Scope, SessionKey, Task};

pub(super) enum Relocation {
    Settled(u64),
    Checking {
        from: SessionKey,
        destination: String,
        observed: u64,
        request: u64,
    },
    Verified {
        change: OpenCodeMove,
        request: u64,
    },
}

impl Default for Relocation {
    fn default() -> Self {
        Self::Settled(0)
    }
}

impl Relocation {
    pub(super) fn pending(&self) -> bool {
        !matches!(self, Self::Settled(_))
    }

    fn destination(&self) -> Option<&str> {
        match self {
            Self::Settled(_) => None,
            Self::Checking { destination, .. } => Some(destination),
            Self::Verified { change, .. } => Some(&change.project.path),
        }
    }

    fn request(&self) -> u64 {
        match self {
            Self::Settled(request)
            | Self::Checking { request, .. }
            | Self::Verified { request, .. } => *request,
        }
    }
}

impl Workspace {
    fn has_open_checkout(&self, path: &str) -> bool {
        self.projects.iter().any(|p| p.path == path)
    }

    pub(super) fn opencode_moved(&mut self, listener: &str, data: &serde_json::Value) {
        let (Some(id), Some(destination)) = (
            data["sessionID"].as_str(),
            data["location"]["directory"].as_str(),
        ) else {
            return;
        };
        let known = self.sessions.iter().position(|s| {
            !s.forgotten()
                && s.scope.provider == Provider::OpenCode
                && s.remote_id.as_deref() == Some(id)
        });
        let open = self.has_open_checkout(destination);
        match known {
            Some(i) => {
                let session = &self.sessions[i];
                if listener != destination
                    && listener != session.scope.project
                    && session.relocation.destination() != Some(listener)
                {
                    return;
                }
                if session.scope.project == destination && !session.relocation.pending() {
                    return;
                }
                if open {
                    self.check_discovered_move(i, destination, self.next_request + 1);
                } else {
                    let source_open = self.project_is_open(&session.scope.project);
                    if session.relocation.pending() {
                        self.sessions[i].relocation = Relocation::Settled(self.next_request);
                        self.pending.retain(|_, p| !matches!(p, Pending::Resume(j) | Pending::History(j) if *j == i));
                        self.sessions[i].cancel_history_read();
                    }

                    if source_open {
                        self.refresh(i);
                    }
                }
            }
            None if open => self.discover_sessions(Scope {
                project: destination.into(),
                provider: Provider::OpenCode,
            }),
            _ => {}
        }
    }

    pub(super) fn check_discovered_move(&mut self, index: usize, destination: &str, observed: u64) {
        let session = &self.sessions[index];

        let barrier = match &session.relocation {
            Relocation::Checking { observed, .. } => *observed,
            other => other.request(),
        };
        if observed <= barrier
            || self.closed_since(destination, observed)
            || !self.has_open_checkout(destination)
            || session.relocation.destination() == Some(destination)
            || session.forgotten()
        {
            return;
        }
        let Some(from) = session
            .key()
            .filter(|k| k.scope.provider == Provider::OpenCode)
        else {
            return;
        };
        let request = self.request(
            Command::VerifyOpenCodeMove {
                from: from.clone(),
                destination: destination.into(),
            },
            Pending::VerifyOpenCodeMove(index),
        );
        self.sessions[index].relocation = Relocation::Checking {
            from,
            destination: destination.into(),
            observed,
            request,
        };
    }

    pub(super) fn finish_move_check(
        &mut self,
        request: u64,
        index: usize,
        result: Result<Reply, String>,
    ) {
        let session = &self.sessions[index];
        let Relocation::Checking {
            request: current,
            from,
            destination,
            observed,
        } = &session.relocation
        else {
            return;
        };
        if request != *current {
            return;
        }
        if !session.forgotten()
            && session.key().as_ref() == Some(from)
            && !self.closed_since(destination, request)
            && self.has_open_checkout(destination)
        {
            match result {
                Ok(Reply::SessionMoved(change))
                    if change.from == *from
                        && change.project.path == *destination
                        && !change.linked_worktree =>
                {
                    if destination == &from.scope.project {
                        self.sessions[index].relocation = Relocation::Settled(request);
                        self.refresh(index);
                    } else {
                        self.verified_move(request, index, change);
                    }
                    return;
                }
                Err(error) => {
                    self.notices.insert(
                        Scope {
                            project: destination.clone(),
                            provider: Provider::OpenCode,
                        },
                        error,
                    );
                }
                _ => {}
            }
        }
        self.sessions[index].relocation = Relocation::Settled(*observed);
    }

    pub(super) fn resume_moved(&mut self, request: u64, index: usize, change: OpenCodeMove) {
        let session = &mut self.sessions[index];
        if session.finish_resume(request) && request > session.relocation.request() {
            self.verified_move(request, index, change);
        }
    }

    fn verified_move(&mut self, request: u64, index: usize, change: OpenCodeMove) {
        let open = self.has_open_checkout(&change.project.path);
        let session = &mut self.sessions[index];
        if change.from.scope.provider != Provider::OpenCode
            || session.key().as_ref() != Some(&change.from)
            || change.project.path == change.from.scope.project
        {
            session.error = Some("Provider returned a different session location".into());
            return;
        }
        session.queue_paused = true;
        if !change.linked_worktree && !open {
            session.error = Some(format!(
                "This OpenCode session is now in '{}'. Open that directory and refresh sessions to reconnect.",
                change.project.path
            ));
            session.relocation = Relocation::Settled(request);
            return;
        }
        session.relocation = Relocation::Verified { change, request };
    }

    pub(super) fn finish_relocation(&mut self, index: usize) {
        let Relocation::Verified { change, request } = &self.sessions[index].relocation else {
            return;
        };

        if self
            .pending
            .values()
            .any(|pending| pending.blocks_relocation(index, &change.from))
        {
            return;
        }
        let request = *request;
        let Relocation::Verified { change, .. } = std::mem::replace(
            &mut self.sessions[index].relocation,
            Relocation::Settled(request),
        ) else {
            unreachable!()
        };
        let open = self.has_open_checkout(&change.project.path);
        if self.sessions[index].forgotten()
            || self.sessions[index].key().as_ref() != Some(&change.from)
            || self.closed_since(&change.project.path, request)
            || (!open
                && (!change.linked_worktree
                    || self.closed_since(&change.from.scope.project, request)))
        {
            return;
        }
        let root = self
            .group_path(if open {
                &change.project.path
            } else {
                &change.from.scope.project
            })
            .to_owned();
        let source_open = self.project_is_open(&change.from.scope.project);

        self.pending
            .retain(|_, pending| !pending.location_read(index));
        let session = &mut self.sessions[index];
        session.scope.project.clone_from(&change.project.path);
        session.observe_archive(change.archived, request);
        session.cancel_history_read();
        session.error = None;
        session.details = None;
        session.shells.clear();
        session.shell_revision += 1;
        session.inbox.revision += 1;
        session.completion_generation += 1;
        session.queue_paused = true;
        let scope = session.scope.clone();
        self.remembered.retain(|_, remembered| *remembered != index);
        self.remembered_checkouts
            .retain(|_, remembered| *remembered != index);
        self.remembered
            .entry(scope.project.clone())
            .or_insert(index);
        self.remembered_checkouts
            .entry(scope.project.clone())
            .or_insert(index);
        if !open {
            self.project_roots
                .insert(change.project.path.clone(), root.clone());
            if source_open {
                self.projects.push(change.project);
            }
        }
        if self.selected == Some(index) {
            self.active_project = Some(scope.project.clone());
            self.remembered.insert(scope.project.clone(), index);
            self.remembered_checkouts
                .insert(scope.project.clone(), index);
            self.remembered.insert(root, index);
        }
        self.refresh(index);
        self.refresh_choices_for(scope.clone());
        self.detail_queue.push(index);
        if open || source_open {
            self.task(Task::Git {
                project: scope.project,
            });
        }
    }
}

impl Pending {
    fn location_read(&self, index: usize) -> bool {
        matches!(self,
            Self::Resume(i) | Self::History(i) | Self::Details(i)
            | Self::Shells(i, _) | Self::Inbox(i, _) if *i == index)
            || matches!(self, Self::VerifyOpenCodeMove(i) if *i == index)
    }

    fn blocks_relocation(&self, index: usize, key: &SessionKey) -> bool {
        match self {
            Self::Fork(
                super::fork::Request::Create { index: i, .. }
                | super::fork::Request::Rewind { index: i, .. },
            )
            | Self::ConversationControl(i, _, _) => *i == index,
            Self::Enqueue(i, _)
            | Self::RemoveQueued(i, _)
            | Self::Control(i)
            | Self::Generate(i, _, _)
            | Self::RegenerateTitle(i, _)
            | Self::Archive(i)
            | Self::DeleteSession(i)
            | Self::Attachment(i, _)
            | Self::Mention { index: i, .. }
            | Self::Start(i)
            | Self::Send(i)
            | Self::Cancel(i) => *i == index,
            Self::Request {
                provider,
                project,
                session,
            } => {
                provider == key.scope.provider.as_str()
                    && project == &key.scope.project
                    && session.as_ref().is_none_or(|id| id == &key.id)
            }
            Self::Task(Task::Shell {
                project,
                session,
                stop: true,
                ..
            }) => project == &key.scope.project && session == &key.id,
            _ => false,
        }
    }
}
