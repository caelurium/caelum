use super::{Lane, Pending, Workspace};
use crate::{
    application::Command,
    remote::{
        Job, MachineId, Operation, Outcome, Speaker, ToolInspection, ToolLoad, ToolSource,
        ToolState,
    },
};

impl Workspace {
    fn vm_tool(
        &self,
        id: &str,
        thread: &str,
        key: &str,
    ) -> Option<(&MachineId, &ToolSource, ToolState)> {
        if self.remote.saved.selected.as_deref() != Some(id)
            || matches!(self.remote.jobs.get(id), Some(Job::Deleting(_)))
        {
            return None;
        }
        let c = self.remote.saved.checkouts.get(id)?;
        if c.thread.editor_key(id) != thread {
            return None;
        }
        let l = c
            .thread
            .lines
            .iter()
            .find(|l| l.key.as_deref() == Some(key))?;
        let Speaker::Tool(state) = l.speaker else {
            return None;
        };
        let source = l.tool_source.as_deref()?;
        if c.thread.conversation() != Some(source.conversation.as_str()) {
            return None;
        }
        Some((&c.machine()?.key, source, state))
    }

    pub fn toggle_vm_tool(&mut self, id: &str, thread: &str, key: &str) {
        if self
            .remote
            .tool
            .as_ref()
            .is_some_and(|p| p.owns(id, thread, key))
        {
            self.remote.tool = None;
        } else {
            self.load_vm_tool(id, thread, key);
        }
    }

    fn load_vm_tool(&mut self, id: &str, thread: &str, key: &str) {
        let Some((machine, source, state)) = self.vm_tool(id, thread, key) else {
            return;
        };
        let source = source.clone();
        let op = Operation::InspectTool(machine.clone(), source.clone());
        let req = self.request(Command::Remote(op), Pending::Remote(Lane::Tool));
        self.remote.tool = Some(ToolInspection {
            checkout: id.into(),
            thread: thread.into(),
            line: key.into(),
            source,
            state,
            load: ToolLoad::Loading(req),
        });
    }

    pub fn dismiss_other_vm_tool(&mut self) -> bool {
        let Some(p) = &self.remote.tool else {
            return false;
        };
        if self.remote.saved.selected.as_ref() == Some(&p.checkout)
            && self
                .remote
                .saved
                .checkouts
                .get(&p.checkout)
                .is_some_and(|c| c.thread.editor_key(&p.checkout) == p.thread)
        {
            return false;
        }
        self.remote.tool = None;
        true
    }

    pub(super) fn refresh_open_vm_tool(&mut self, id: &str) {
        let Some(p) = self.remote.tool.as_ref().filter(|p| p.checkout == id) else {
            return;
        };
        match self.vm_tool(id, &p.thread, &p.line) {
            None => self.remote.tool = None,
            Some((_, source, state)) if source != &p.source || state != p.state => {
                let (thread, key) = (p.thread.clone(), p.line.clone());
                self.load_vm_tool(id, &thread, &key);
            }
            _ => {}
        }
    }

    pub(super) fn complete_vm_tool(&mut self, req: u64, result: Result<Outcome, String>) {
        let Some(p) = &self.remote.tool else { return };
        if !matches!(p.load,ToolLoad::Loading(r) if req==r) {
            return;
        }
        if !self
            .vm_tool(&p.checkout, &p.thread, &p.line)
            .is_some_and(|(_, src, state)| src == &p.source && state == p.state)
        {
            self.remote.tool = None;
            return;
        }
        self.remote.tool.as_mut().unwrap().load = match result {
            Ok(Outcome::ToolOutput(body)) => ToolLoad::Ready(body),
            Err(e) => ToolLoad::Failed(e),
            _ => ToolLoad::Failed("Unexpected Boat tool response".into()),
        };
    }
}
