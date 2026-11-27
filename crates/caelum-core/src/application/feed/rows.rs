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
    General {
        archived: bool,
    },
    Overview,
    Dashboard {
        archived: bool,
    },
}

type FeedRow = (Reverse<(bool, bool, Option<u64>)>, FeedSession);

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

    /// Active conversations come first across local and VM sessions, newest activity first within each group.
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

    /// Unbound General chats ordered like the sidebar (pinned, then active, then newest).
    pub fn general_sidebar_sessions(&self, query: &str, archived: bool) -> Vec<FeedSession> {
        let mut rows = self.feed_rows(Feed::General { archived }, query);
        rows.sort_unstable();
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// Conversations with live work or a pending request, in stable identity order across open projects.
    pub fn overview_sessions(&self, query: &str) -> Vec<FeedSession> {
        let mut rows = self.feed_rows(Feed::Overview, query);
        rows.sort_unstable_by(|(_, a), (_, b)| a.cmp(b));
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// All non-forgotten conversations across open projects, ordered by recency.
    pub fn dashboard_sessions(&self, query: &str, archived: bool) -> Vec<FeedSession> {
        let mut rows = self.feed_rows(Feed::Dashboard { archived }, query);
        rows.sort_unstable();
        rows.into_iter().map(|(_, row)| row).collect()
    }

    /// Count of non-archived dashboard sessions currently waiting for input.
    pub fn waiting_session_count(&self) -> usize {
        let includes = |path: &str| self.project_is_open(path);
        let local = self
            .matching_local_sessions(&includes, true, "", false)
            .filter(|(i, _)| {
                self.session_bucket(&FeedSession::Local(*i)) == super::SessionBucket::Waiting
            })
            .count();
        let mut vm = 0;
        for (id, c) in &self.remote.saved.checkouts {
            if !includes(&c.source.project) {
                continue;
            }
            for t in c.threads() {
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
                if self.session_bucket(&FeedSession::vm(id, t)) == super::SessionBucket::Waiting {
                    vm += 1;
                }
            }
        }
        local + vm
    }

    fn feed_rows(&self, feed: Feed<'_>, query: &str) -> Vec<FeedRow> {
        let includes = |path: &str| match feed {
            Feed::Sidebar { project, .. } => self.feed_includes_project(path, project),
            Feed::General { .. } => false,
            Feed::Overview | Feed::Dashboard { .. } => self.project_is_open(path),
        };
        let include_unbound = match feed {
            Feed::Sidebar { project, .. } => self.feed_project(project).is_none(),
            Feed::General { .. } | Feed::Overview | Feed::Dashboard { .. } => true,
        };
        let archived = matches!(
            feed,
            Feed::Sidebar { archived: true, .. }
                | Feed::General { archived: true }
                | Feed::Dashboard { archived: true }
        );
        let active_only = matches!(feed, Feed::Overview);
        let query = query.trim().to_lowercase();
        let mut rows: Vec<_> = self
            .matching_local_sessions(&includes, include_unbound, &query, archived)
            .filter_map(|(i, s)| {
                let active = !archived && self.overview_activity(s).is_some();
                let pinned_order = !archived && s.pinned;
                let activity_order =
                    matches!(feed, Feed::Sidebar { .. } | Feed::General { .. }) && active;
                (!active_only || active).then_some((
                    Reverse((pinned_order, activity_order, s.last_activity)),
                    FeedSession::Local(i),
                ))
            })
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
                    let active = t.turn != Turn::Idle;
                    let activity_order = matches!(feed, Feed::Sidebar { .. }) && active;
                    if active_only && !active {
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
                        rows.push((
                            Reverse((false, activity_order, t.last_activity)),
                            FeedSession::vm(id, t),
                        ));
                    }
                }
            }
        }
        rows
    }
}
