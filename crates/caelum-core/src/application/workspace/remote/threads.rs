use super::Workspace;
use crate::application::Provider;
use crate::remote::{Job, Phase, Thread, Turn};

impl Workspace {
    pub fn can_switch_vm_thread(&self, id: &str) -> bool {
        self.remote.saved.checkouts.get(id).is_some_and(|c| {
            c.thread.turn == Turn::Idle
                && !matches!(
                    c.phase,
                    Phase::Create(_)
                        | Phase::Prepare {
                            new_branch: Some(_),
                            ..
                        }
                )
        }) && !matches!(
            self.remote.jobs.get(id),
            Some(Job::Busy(_) | Job::Wait(_) | Job::Deleting(_))
        ) && !matches!(self.remote.feeds.get(id), Some(Job::Busy(_) | Job::Wait(_)))
            && !matches!(self.remote.lifetimes.get(id), Some(Job::Busy(_)))
            && !self.remote.interrupts.contains_key(id)
            && !self.publishing(id)
    }

    pub fn can_start_vm_session(&self, id: &str) -> bool {
        self.can_switch_vm_thread(id)
            && matches!(self.remote.saved.checkouts[id].phase, Phase::Ready { .. })
    }

    /// Opening a blank editor never submits a prompt or changes the VM branch.
    pub fn new_vm_session(&mut self, id: &str) -> bool {
        if !self.can_start_vm_session(id) {
            return false;
        }
        let c = self.remote.saved.checkouts.get_mut(id).unwrap();
        if !c.thread.is_blank() {
            c.open_thread(Thread {
                local_id: Some(uuid::Uuid::new_v4().to_string()),
                provider: c.thread.provider,
                ..Default::default()
            });
        }
        self.remote.feeds.remove(id);
        if self.remote.saved.selected.as_deref() != Some(id) {
            self.select_remote(id);
        } else if matches!(self.remote.jobs.get(id), Some(Job::Failed(_))) {
            self.refresh_remote(id);
        }
        true
    }

    /// The next Send uses this harness in the same Boat conversation.
    pub fn set_vm_provider(&mut self, id: &str, owner: &str, provider: Provider) -> bool {
        if self.remote.saved.selected.as_deref() != Some(id) || !self.can_switch_vm_thread(id) {
            return false;
        }
        let thread = &mut self.remote.saved.checkouts.get_mut(id).unwrap().thread;
        if thread.editor_key(id) != owner || thread.provider == provider {
            return false;
        }
        thread.provider = provider;
        self.remote.feeds.remove(id);
        true
    }

    pub fn switch_vm_thread(&mut self, id: &str, thread: &str) -> bool {
        if self.remote.saved.selected.as_deref() != Some(id) || !self.activate_vm_thread(id, thread)
        {
            return false;
        }
        if matches!(self.remote.jobs.get(id), Some(Job::Failed(_))) {
            self.refresh_remote(id);
        }
        true
    }

    pub(super) fn activate_vm_thread(&mut self, id: &str, thread: &str) -> bool {
        if !self.can_switch_vm_thread(id) {
            return false;
        }
        let c = self.remote.saved.checkouts.get_mut(id).unwrap();
        let Some(other) = c
            .other_threads
            .iter_mut()
            .find(|t| t.editor_key(id) == thread)
        else {
            return false;
        };
        std::mem::swap(&mut c.thread, other);
        self.remote.feeds.remove(id);
        true
    }
}
