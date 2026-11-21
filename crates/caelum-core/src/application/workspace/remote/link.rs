use super::Workspace;
use crate::remote::{Job, VmLink};

impl Workspace {
    pub fn linked_vm(&self, index: usize) -> Option<&VmLink> {
        let link = self.sessions.get(index)?.vm_link.as_ref()?;
        if self.inline_link(link).is_none() {
            link.resolve(&self.remote.saved)?;
        }
        Some(link)
    }

    pub fn can_open_linked_vm(&self, index: usize) -> bool {
        self.linked_vm(index)
            .is_some_and(|link| self.can_open_vm_thread(link))
    }

    pub fn open_linked_vm(&mut self, index: usize) -> bool {
        let Some(link) = self.linked_vm(index).cloned() else {
            return false;
        };
        self.open_vm_thread(&link)
    }

    pub fn can_open_vm_thread(&self, link: &VmLink) -> bool {
        if self.inline_link(link).is_some() {
            return !matches!(self.remote.jobs.get(&link.checkout), Some(Job::Deleting(_)));
        }
        link.resolve(&self.remote.saved).is_some_and(|(c, _)| {
            !matches!(self.remote.jobs.get(&link.checkout), Some(Job::Deleting(_)))
                && (c.thread.editor_key(&link.checkout) == link.thread
                    || self.can_switch_vm_thread(&link.checkout))
        })
    }

    /// Open the exact saved editor, including a parked draft with no Boat conversation yet.
    pub fn open_vm_thread(&mut self, link: &VmLink) -> bool {
        if !self.can_open_vm_thread(link) {
            return false;
        }
        if let Some(i) = self.inline_link(link) {
            self.select(i);
            return true;
        }
        let current = self.remote.saved.checkouts[&link.checkout]
            .thread
            .editor_key(&link.checkout)
            == link.thread;
        if self.remote.saved.selected.as_ref() == Some(&link.checkout) {
            return current || self.switch_vm_thread(&link.checkout, &link.thread);
        }
        if !current && !self.activate_vm_thread(&link.checkout, &link.thread) {
            return false;
        }
        self.select_remote(&link.checkout);
        true
    }
}
