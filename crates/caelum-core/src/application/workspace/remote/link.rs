use super::Workspace;
use crate::remote::VmLink;

impl Workspace {
    pub fn linked_vm(&self, index: usize) -> Option<&VmLink> {
        let link = self.sessions.get(index)?.vm_link.as_ref()?;
        let c = self.remote.saved.checkouts.get(&link.checkout)?;
        (c.thread.editor_key(&link.checkout) == link.thread
            || c.other_threads
                .iter()
                .any(|t| t.editor_key(&link.checkout) == link.thread))
        .then_some(link)
    }

    pub fn can_open_linked_vm(&self, index: usize) -> bool {
        self.linked_vm(index).is_some_and(|link| {
            self.remote.saved.checkouts[&link.checkout]
                .thread
                .editor_key(&link.checkout)
                == link.thread
                || self.can_switch_vm_thread(&link.checkout)
        })
    }

    pub fn open_linked_vm(&mut self, index: usize) -> bool {
        if !self.can_open_linked_vm(index) {
            return false;
        }
        let link = self.linked_vm(index).unwrap().clone();
        if self.remote.saved.checkouts[&link.checkout]
            .thread
            .editor_key(&link.checkout)
            != link.thread
            && !self.activate_vm_thread(&link.checkout, &link.thread)
        {
            return false;
        }
        self.select_remote(&link.checkout);
        true
    }
}
