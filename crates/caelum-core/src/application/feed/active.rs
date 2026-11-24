use crate::application::{subagents, FeedSession, Provider, Session, Workspace};
use crate::remote::Turn;

/// High-level lifecycle grouping for conversations in the Overview screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionBucket {
    Waiting,
    Running,
    Settled,
}

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
    /// Classify a feed session into Waiting (needs user action), Running, or Settled.
    pub fn session_bucket(&self, id: &FeedSession) -> SessionBucket {
        match id {
            FeedSession::Local(index) => {
                let Some(session) = self.sessions.get(*index) else {
                    return SessionBucket::Settled;
                };
                match self.overview_activity(session) {
                    Some(OverviewActivity::Question | OverviewActivity::Approval(_)) => {
                        SessionBucket::Waiting
                    }
                    Some(
                        OverviewActivity::Agent
                        | OverviewActivity::Command
                        | OverviewActivity::SideQuestion
                        | OverviewActivity::Queued(_),
                    ) => SessionBucket::Running,
                    None => {
                        if self.requested_changes(session).is_some()
                            || (session.queue_paused && !session.queue.is_empty())
                            || session.error.is_some()
                            || session.conversation.failure.notice().is_some()
                        {
                            SessionBucket::Waiting
                        } else {
                            SessionBucket::Settled
                        }
                    }
                }
            }
            FeedSession::Vm(link) => match link.resolve(&self.remote.saved) {
                Some((_, thread)) if thread.turn != Turn::Idle => SessionBucket::Running,
                _ => SessionBucket::Settled,
            },
        }
    }

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
