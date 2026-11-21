use super::Workspace;
use crate::remote::{Thread, VmLink};
use std::cmp::Reverse;

/// Feed identity never depends on a row position, title, or provider conversation ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeedSession {
    Local(usize),
    Vm(VmLink),
}

impl FeedSession {
    fn vm(checkout: &str, thread: &Thread) -> Self {
        Self::Vm(VmLink {
            checkout: checkout.to_owned(),
            thread: thread.editor_key(checkout).to_owned(),
        })
    }

    pub fn last_activity(&self, app: &Workspace) -> Option<u64> {
        match self {
            Self::Local(i) => app.sessions.get(*i)?.last_activity,
            Self::Vm(link) => link.resolve(&app.remote.saved)?.1.last_activity,
        }
    }
}

impl Workspace {
    pub fn selected_feed_session(&self) -> Option<FeedSession> {
        if let Some(i) = self.selected {
            return Some(FeedSession::Local(i));
        }
        let checkout = self.remote.saved.selected.as_ref()?;
        let c = self.remote.saved.checkouts.get(checkout)?;
        Some(FeedSession::vm(checkout, &c.thread))
    }

    /// Local sessions and saved VM editors share one scoped, activity-ordered list.
    pub fn sidebar_sessions(
        &self,
        project: Option<&str>,
        query: &str,
        archived: bool,
    ) -> Vec<FeedSession> {
        let query = query.trim().to_lowercase();
        let mut rows: Vec<_> = self
            .matching_local_sessions(project, &query, archived)
            .map(|(i, s)| (Reverse(s.last_activity), FeedSession::Local(i)))
            .collect();
        if !archived {
            for (id, c) in &self.remote.saved.checkouts {
                if !self.feed_includes_project(&c.source.project, project) {
                    continue;
                }
                let matches_vm = query.is_empty() || {
                    let root = self.group_path(&c.source.project);
                    let project = self
                        .projects
                        .iter()
                        .find(|p| p.path == root)
                        .map_or("", |p| p.name.as_str());
                    format!(
                        "VM {} {} {} {} {project}",
                        c.title(),
                        c.source.repository,
                        c.source.project,
                        c.branch().unwrap_or("")
                    )
                    .to_lowercase()
                    .contains(&query)
                };
                for t in c.threads() {
                    if t.is_blank()
                        && t.local_id.is_none()
                        && self.sessions.iter().any(|s| {
                            !s.forgotten
                                && crate::remote::native::checkout_id(&s.scope.project)
                                    == Some(id.as_str())
                        })
                    {
                        continue;
                    }
                    if matches_vm
                        || t.title().to_lowercase().contains(&query)
                        || t.provider.as_str().contains(&query)
                    {
                        rows.push((Reverse(t.last_activity), FeedSession::vm(id, t)));
                    }
                }
            }
        }
        rows.sort_unstable();
        rows.into_iter().map(|(_, row)| row).collect()
    }
}
