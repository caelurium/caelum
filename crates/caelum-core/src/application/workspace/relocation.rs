//! Reconnect provider-moved sessions without changing their stable local slots.
use super::{Pending, Workspace};
use crate::application::{Provider, SessionKey, Task};
use caelum_protocol::ProjectInfo;

pub(super) struct Relocation {
    from: SessionKey,
    project: ProjectInfo,
    request: u64,
}

impl Workspace {
    pub(super) fn relocate_session(
        &mut self,
        request: u64,
        index: usize,
        from: SessionKey,
        project: ProjectInfo,
    ) {
        let session = &mut self.sessions[index];
        session.loading = false;
        if from.scope.provider != Provider::OpenCode
            || session.key().as_ref() != Some(&from)
            || project.path == from.scope.project
        {
            session.error = Some("Provider returned a different session location".into());
            return;
        }
        session.relocation = Some(Relocation {
            from,
            project,
            request,
        });
    }

    pub(super) fn finish_relocation(&mut self, index: usize) {
        let Some(change) = &self.sessions[index].relocation else {
            return;
        };
        // Local operations keep their original owner until they finish. This
        // also covers mutations waiting for the durable submission snapshot.
        if self
            .pending
            .values()
            .any(|pending| pending.blocks_relocation(index, &change.from))
        {
            return;
        }
        let change = self.sessions[index].relocation.take().unwrap();
        if self.sessions[index].forgotten
            || self.sessions[index].key().as_ref() != Some(&change.from)
            || self.closed_since(&change.from.scope.project, change.request)
        {
            return;
        }
        let root = self.group_path(&change.from.scope.project).to_owned();
        let open = self.project_is_open(&change.from.scope.project);
        // Completions from the previous checkout cannot change the new scope's
        // history, activity, inbox, or branch metadata, including error replies.
        self.pending
            .retain(|_, pending| !pending.location_read(index));
        let session = &mut self.sessions[index];
        session.scope.project.clone_from(&change.project.path);
        session.loading = false;
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
        self.remembered.insert(change.project.path.clone(), index);
        self.remembered_checkouts
            .insert(change.project.path.clone(), index);
        self.project_roots
            .insert(change.project.path.clone(), root.clone());
        if open && !self.projects.iter().any(|p| p.path == change.project.path) {
            self.projects.push(change.project);
        }
        if self.selected == Some(index) {
            self.active_project = Some(scope.project.clone());
            self.remembered.insert(root, index);
        }
        self.refresh(index);
        self.refresh_choices_for(scope.clone());
        self.detail_queue.push(index);
        if open {
            self.task(Task::Git {
                project: scope.project,
            });
        }
    }
}

impl Pending {
    fn location_read(&self, index: usize) -> bool {
        matches!(self,
            Self::Resume(i, _) | Self::History(i, _, _) | Self::Details(i)
            | Self::Shells(i, _) | Self::Inbox(i, _) if *i == index)
    }

    fn blocks_relocation(&self, index: usize, key: &SessionKey) -> bool {
        match self {
            Self::Enqueue(i, _)
            | Self::RemoveQueued(i, _)
            | Self::Control(i, _)
            | Self::Generate(i, _, _)
            | Self::RegenerateTitle(i, _)
            | Self::Archive(i, _, _, _)
            | Self::DeleteSession(i)
            | Self::Attachment(i, _)
            | Self::Mention { index: i, .. }
            | Self::Start(i, _)
            | Self::Send(i, _, _)
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
