use crate::application::{subagents, Provider, Session, Workspace};

/// The live work or pending request that keeps a local or native VM conversation in the overview.
pub enum OverviewActivity<'a> {
    Question,
    Approval(&'a str),
    Agent,
    Command,
    SideQuestion,
    Queued(&'a str),
}

impl Workspace {
    /// A project-wide approval can interrupt active work, but does not activate idle sessions.
    pub fn overview_activity<'a>(&'a self, session: &'a Session) -> Option<OverviewActivity<'a>> {
        if self.has_question(session) {
            return Some(OverviewActivity::Question);
        }
        let work = session.overview_work();
        self.pending_approval(session, work.is_some())
            .map(|p| OverviewActivity::Approval(&p.summary))
            .or(work)
    }
}

impl Session {
    fn overview_work(&self) -> Option<OverviewActivity<'_>> {
        if self.disconnected {
            return None;
        }
        if self.working()
            || self.stopping()
            || !self.shells.is_empty()
            || (self.scope.provider == Provider::OpenCode
                && subagents::running_entries(&self.conversation.entries)
                    .next_back()
                    .is_some())
        {
            return Some(OverviewActivity::Agent);
        }
        if self.commanding() {
            return Some(OverviewActivity::Command);
        }
        if self.generating {
            return Some(OverviewActivity::SideQuestion);
        }
        self.inbox
            .queued_text()
            .or_else(|| {
                self.queue
                    .first()
                    .filter(|_| !self.queue_paused)
                    .map(|p| p.text.as_str())
            })
            .map(OverviewActivity::Queued)
    }
}
