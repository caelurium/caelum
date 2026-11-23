use super::Workspace;
use crate::remote::{Thread, Turn, VmLink};
use std::cmp::Reverse;

/// Feed identity never depends on a row position, title, or provider conversation ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FeedSession {
    Local(usize),
    Vm(VmLink),
}

enum Feed<'a> {
    Sidebar {
        project: Option<&'a str>,
        archived: bool,
    },
    Overview,
    Dashboard {
        archived: bool,
    },
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
        let mut rows = self.feed_rows(Feed::Sidebar { project, archived }, query);
        rows.sort_unstable();
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// Conversations with live work or a pending request, in stable identity order across open projects.
    pub fn overview_sessions(&self, query: &str) -> Vec<FeedSession> {
        let mut rows: Vec<_> = self
            .feed_rows(Feed::Overview, query)
            .into_iter()
            .map(|(_, row)| row)
            .collect();
        rows.sort_unstable();
        rows
    }

    /// All non-forgotten conversations across open projects, ordered by recency.
    pub fn dashboard_sessions(&self, query: &str, archived: bool) -> Vec<FeedSession> {
        let mut rows = self.feed_rows(Feed::Dashboard { archived }, query);
        rows.sort_unstable();
        rows.into_iter().map(|(_, row)| row).collect()
    }

    fn feed_rows(&self, feed: Feed<'_>, query: &str) -> Vec<(Reverse<Option<u64>>, FeedSession)> {
        let includes = |path: &str| match feed {
            Feed::Sidebar { project, .. } => self.feed_includes_project(path, project),
            Feed::Overview | Feed::Dashboard { .. } => self.project_is_open(path),
        };
        let archived = matches!(
            feed,
            Feed::Sidebar { archived: true, .. } | Feed::Dashboard { archived: true }
        );
        let active_only = matches!(feed, Feed::Overview);
        let query = query.trim().to_lowercase();
        let mut rows: Vec<_> = self
            .matching_local_sessions(&includes, &query, archived)
            .filter(|(_, s)| !active_only || self.overview_activity(s).is_some())
            .map(|(i, s)| (Reverse(s.last_activity), FeedSession::Local(i)))
            .collect();
        if !archived {
            for (id, c) in &self.remote.saved.checkouts {
                if !includes(&c.source.project) {
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
                    if active_only && t.turn == Turn::Idle {
                        continue;
                    }
                    if t.is_blank()
                        && t.local_id.is_none()
                        && self.sessions.iter().any(|s| {
                            !s.forgotten()
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
        rows
    }
}
