use super::{submission::Origin, Pending, Workspace};
use crate::application::{inbox::Mutation, Command, Provider};

impl Workspace {
    pub fn resume_queue(&mut self, index: usize) {
        self.sessions[index].queue_paused = false;
        self.sessions[index].error = None;
        if !self.sessions[index].loaded() || self.sessions[index].disconnected {
            self.refresh(index);
        }
        self.advance_queue(index);
    }

    pub fn refresh_inbox(&mut self, index: usize) {
        let session = &self.sessions[index];
        if session.scope.provider != Provider::OpenCode
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::Inbox(i, _) if *i == index))
        {
            return;
        }
        if let Some(key) = session.key() {
            self.request(
                Command::Inbox(key),
                Pending::Inbox(index, session.inbox.revision),
            );
        }
    }

    pub fn retry_queued(&mut self, index: usize, id: &str) {
        if self.sessions[index]
            .inbox
            .items
            .iter()
            .any(|item| item.id == id && item.removing())
        {
            self.remove_queued(index, id);
            return;
        }
        let session = &mut self.sessions[index];
        let Some(key) = session.key() else {
            return;
        };
        let Some(item) = session
            .inbox
            .items
            .iter()
            .find(|v| v.id == id && !v.pending() && v.error().is_some())
        else {
            return;
        };
        let text = item.text.clone();
        let request = self.request(
            Command::Enqueue(key, id.into(), text),
            Pending::Enqueue(index, id.into()),
        );
        self.sessions[index]
            .inbox
            .start_mutation(id, request, Mutation::Enqueue);
    }

    pub fn remove_queued(&mut self, index: usize, id: &str) {
        let session = &mut self.sessions[index];
        let Some(key) = session.key() else {
            return;
        };
        if !session
            .inbox
            .items
            .iter()
            .any(|v| v.id == id && !v.pending())
        {
            return;
        }
        let request = self.request(
            Command::RemoveQueued(key, id.into()),
            Pending::RemoveQueued(index, id.into()),
        );
        self.sessions[index]
            .inbox
            .start_mutation(id, request, Mutation::Remove);
    }

    pub(super) fn advance_queue(&mut self, index: usize) {
        if self
            .pending
            .values()
            .any(|p| matches!(p, Pending::Inbox(i, _) if *i == index))
        {
            return;
        }
        let session = &mut self.sessions[index];
        if !session.queue_paused
            && session.inbox.items.is_empty()
            && session.can_send()
            && !session.queue.is_empty()
            && session.error.is_none()
        {
            let queued = session.queue.remove(0);
            let dirty = session.options_dirty || session.options != queued.options;
            self.submit_prompt(index, queued, Origin::Queue);
            self.sessions[index].options_dirty = dirty;
        }
    }
}
