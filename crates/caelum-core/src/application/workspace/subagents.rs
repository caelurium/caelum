use super::{Session, Workspace};
use crate::application::{subagents::Subagent, Provider};

impl Workspace {
    /// Resolve the current task again at activation, then use the existing
    /// scoped resume path. Merely rendering a task never loads a child session.
    pub fn open_subagent(&mut self, parent: usize, entry_id: &str) -> Option<usize> {
        let owner = self.sessions.get(parent)?;
        if owner.forgotten || owner.scope.provider != Provider::OpenCode {
            return None;
        }
        let entry = owner
            .conversation
            .entries
            .iter()
            .find(|e| e.id == entry_id)?;
        let task = Subagent::from_entry(entry)?;
        let child = task.session_id?;
        if owner.remote_id.as_deref() == Some(&child) {
            return None;
        }
        let scope = owner.scope.clone();
        let existing = self
            .sessions
            .iter()
            .position(|s| s.scope == scope && s.remote_id.as_deref() == Some(&child));
        let index = if let Some(index) = existing {
            if self.sessions[index].forgotten || self.sessions[index].archived {
                return None;
            }
            index
        } else {
            let mut session = Session::new(scope);
            session.remote_id = Some(child);
            session.title = task.description;
            session.invalidate_history();
            let index = self.sessions.len();
            self.sessions.push(session);
            index
        };
        self.select(index);
        Some(index)
    }

    pub fn subagent_parent(&self, child: usize) -> Option<usize> {
        let child = self.sessions.get(child)?;
        let id = child.remote_id.as_deref()?;
        self.sessions.iter().position(|parent| {
            !parent.forgotten
                && !parent.archived
                && parent.scope == child.scope
                && parent.remote_id.as_deref() != Some(id)
                && parent
                    .conversation
                    .entries
                    .iter()
                    .any(|entry| crate::application::subagents::child_id(entry) == Some(id))
        })
    }
}
