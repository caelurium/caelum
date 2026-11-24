//! Session-first navigation. Feed controls and timers belong to this feature.
mod badge;
mod projects;
mod row;
mod session;
mod vm;

use std::rc::Rc;

use caelum_core::application::{
    FeedSession, SessionBucket, SessionScope, Workspace as Application,
};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, icon, row, text, text_input};
use rune_kit::{Kit, ScrollBar, glyphs, shell::SidebarHeader};

use super::{live, live_mut};
use crate::{PLUS, Workspace, context::EditMenu, quiet_button, ui};

enum Recency {
    Paused {
        displayed_at: u64,
    },
    Showing {
        displayed_at: u64,
        wake_at: Option<u64>,
    },
}

impl Recency {
    fn displayed_at(&self) -> u64 {
        match self {
            Self::Paused { displayed_at } | Self::Showing { displayed_at, .. } => *displayed_at,
        }
    }
}

pub struct State {
    badges: badge::State,
    discovered: std::collections::HashSet<caelum_core::application::Scope>,
    recency: Recency,
    visible: Vec<FeedSession>,
    query: Option<(Option<String>, String, bool)>,
    request_scope: Option<(SessionScope, Option<String>)>,
    selected: Option<FeedSession>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            badges: Default::default(),
            discovered: Default::default(),
            recency: Recency::Paused {
                displayed_at: caelum_core::application::presentation::now(),
            },
            visible: Vec::new(),
            query: None,
            request_scope: None,
            selected: None,
        }
    }
}

impl State {
    pub(crate) fn project_ink(&self, app: &Application, path: &str, k: &Kit) -> rune::Color {
        self.badges.ink(app, path, k)
    }

    pub(crate) fn project_badge(&self, app: &Application, path: &str, k: &Kit) -> El {
        self.badges.view(app, path, k)
    }

    pub fn displayed_at(&self) -> u64 {
        self.recency.displayed_at()
    }

    pub fn set_display_clock(&mut self, displayed_at: u64) {
        self.recency = Recency::Paused { displayed_at };
    }

    pub fn discover(&mut self, app: &mut Application) {
        self.discovered
            .retain(|scope| app.project_is_open(&scope.project));
        let scopes: Vec<_> = app
            .projects
            .iter()
            .flat_map(|p| {
                caelum_core::application::Provider::ALL.map(|provider| {
                    caelum_core::application::Scope {
                        project: p.path.clone(),
                        provider,
                    }
                })
            })
            .collect();
        for scope in scopes {
            if self.discovered.insert(scope.clone()) {
                app.discover_sessions(scope);
            }
        }
    }
    pub fn close(&mut self) {}
    pub fn hidden(&mut self) {
        self.visible.clear();
        self.set_display_clock(self.displayed_at());
    }
    pub fn poll(
        &mut self,
        app: &mut Application,
        query: &str,
        archived: bool,
        tx: &mut Tx,
        clock: bool,
    ) -> bool {
        let mut changed = self.badges.sync(app);
        let project = app.feed_project(None).map(str::to_owned);
        let selected = app.selected_feed_session();
        let request_scope = (app.preferences.session_scope, project.clone());
        if self.request_scope.as_ref() != Some(&request_scope) {
            app.refresh_feed_pull_requests(None);
            self.request_scope = Some(request_scope);
        } else if self.selected != selected
            && let Some(path) = app.active_project.clone()
            && app.feed_includes_project(&path, None)
        {
            app.refresh_feed_pull_requests(Some(&path));
        }
        let query = (project, query.to_owned(), archived);
        if self.query.as_ref() != Some(&query) {
            let rows = app.sidebar_sessions(None, &query.1, archived);
            self.visible.retain(|i| rows.contains(i));
            tx.scroll_to_y("sidebar-scroll", 0.0);
            self.query = Some(query);
        }
        if self.selected != selected {
            self.selected = selected;
            if let Some(row) = &self.selected {
                tx.scroll_into_view(row::key(row));
            }
            changed = true;
        }
        if clock && !app.window_background {
            let now = caelum_core::application::presentation::now();
            changed |= self
                .visible
                .iter()
                .filter_map(|row| row.last_activity(app))
                .any(|at| age(Some(at), self.displayed_at()).0 != age(Some(at), now).0);
            let deadline = self
                .visible
                .iter()
                .filter_map(|row| age(row.last_activity(app), now).1)
                .min();
            if let Some(at) = deadline
                && !matches!(self.recency, Recency::Showing { wake_at: Some(wake), .. } if wake == at && now < wake)
            {
                tx.wake_after(std::time::Duration::from_millis(
                    at.saturating_sub(now).max(1),
                ));
            }
            self.recency = Recency::Showing {
                displayed_at: now,
                wake_at: deadline,
            };
        } else {
            self.set_display_clock(self.displayed_at());
        }
        changed
    }
}

impl super::Live {
    pub(crate) fn reveal_new_session(&mut self) {
        self.feed.selected = None;
        self.panels.sessions.clear();
        self.panels.archived = false;
    }
}

fn age(at: Option<u64>, now: u64) -> (String, Option<u64>) {
    let Some(at) = at else {
        return (String::new(), None);
    };
    let elapsed = now.saturating_sub(at);
    let (unit, suffix) = if elapsed < 3_600_000 {
        (60_000, "m")
    } else if elapsed < 86_400_000 {
        (3_600_000, "h")
    } else {
        (86_400_000, "d")
    };
    let count = elapsed / unit;
    (
        if elapsed < 60_000 {
            "now".into()
        } else {
            format!("{count}{suffix}")
        },
        Some(at.saturating_add((count + 1).saturating_mul(unit))),
    )
}

pub fn sidebar(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let rows = Rc::new(
        l.app
            .sidebar_sessions(None, &l.panels.sessions.text, l.panels.archived),
    );
    let (keys, builders, visible) = (rows.clone(), rows.clone(), rows.clone());
    let kit = k.clone();
    let sessions = if rows.is_empty() {
        ui::caption(
            if l.app.projects.is_empty() {
                "Open a project to get started"
            } else if !l.panels.sessions.text.is_empty() {
                "No matching sessions"
            } else if l.panels.archived {
                "No archived sessions"
            } else {
                "No sessions in this project"
            },
            k,
        )
        .padding(12.0)
    } else {
        col()
            .w_full()
            .key("sidebar-list")
            .role("list")
            .label("Sessions")
            .focus_group()
            .focus_follows()
            .growth(rune::GrowthSpec::default())
            .virtual_rows(
                rows.len(),
                row::HEIGHT,
                move |_: &Workspace, i| row::key(&keys[i]).into(),
                move |s: &Workspace, i| match &builders[i] {
                    FeedSession::Local(index) => session::view(s, &kit, *index, i, builders.len()),
                    FeedSession::Vm(link) => vm::view(s, &kit, link, i, builders.len()),
                },
            )
            .virtual_rows_version(rows.as_ref())
            .on_visible_rows(move |s: &mut Workspace, range| {
                live_mut(s).feed.visible = range.filter_map(|i| visible.get(i).cloned()).collect();
                false
            })
    };
    let waiting = l
        .app
        .dashboard_sessions("", false)
        .iter()
        .filter(|id| l.app.session_bucket(id) == SessionBucket::Waiting)
        .count();
    let overview_selected = l.panels.screen == crate::panels::Screen::Overview;
    let overview_nav = k
        .interactive(
            row()
                .w_full()
                .h(30.0)
                .px(10.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .fill(if overview_selected {
                    k.c.hover
                } else {
                    rune::Color::TRANSPARENT
                })
                .child(
                    icon(&crate::overview::NAV_ICON, 13.0)
                        .color(if overview_selected {
                            k.c.text_accent
                        } else {
                            k.c.icon_muted
                        })
                        .a11y_hidden(),
                )
                .child(
                    text("Overview")
                        .font(if overview_selected {
                            k.ty.ui_medium
                        } else {
                            k.ty.ui
                        })
                        .color(if overview_selected {
                            k.c.text_accent
                        } else {
                            k.c.text_muted
                        })
                        .grow(1.0)
                        .min_w(0.0),
                )
                .children((waiting > 0).then(|| {
                    text(waiting.to_string())
                        .font(rune::TextStyle::new(10.5, 600.0))
                        .color(k.c.warning.fg)
                        .px(6.0)
                        .py(1.0)
                        .radius(9.0)
                        .fill(k.alpha(k.c.warning.fg, 0.14))
                })),
        )
        .role("tab")
        .label("Overview")
        .help(ui::shortcut("Overview · ⌘J"))
        .focusable(true)
        .focus_selected(overview_selected)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press(crate::overview::open_from_nav)
        .key("nav-overview");
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(4.0)
        .label("Projects and sessions")
        .child(
            ui::highlighted_tab("Home", l.panels.screen == crate::panels::Screen::Home, k)
                .w_full()
                .on_press(crate::home::open)
                .key("nav-home")
                .label("Home"),
        )
        .child(overview_nav)
        .child(projects::view(s, k))
        .child(ui::divider(k))
        .child(
            SidebarHeader::new(if l.panels.archived {
                "Archived sessions"
            } else {
                "Sessions"
            })
            .count(rows.len())
            .action(
                row()
                    .gap(1.0)
                    .child(
                        quiet_button(&PLUS, "New session · ⌘N")
                            .on_press(|s: &mut Workspace| s.new_session())
                            .build(k)
                            .key("new-session"),
                    )
                    .child(
                        quiet_button(
                            &ui::ARCHIVE,
                            if l.panels.archived {
                                "Show active sessions"
                            } else {
                                "Show archived sessions"
                            },
                        )
                        .expanded(l.panels.archived)
                        .on_press(|s: &mut Workspace| {
                            let p = &mut live_mut(s).panels;
                            p.archived = !p.archived;
                        })
                        .build(k)
                        .key("toggle-archived"),
                    )
                    .child(
                        quiet_button(&ui::REFRESH, "Refresh sessions")
                            .disabled(l.app.projects.is_empty())
                            .on_press(|s: &mut Workspace| {
                                let l = live_mut(s);
                                l.app.refresh_project();
                                l.app.refresh_feed_pull_requests(None);
                                l.app.discover_open_sessions();
                                l.app.refresh_open_sessions();
                            })
                            .build(k)
                            .key("refresh-sessions"),
                    ),
            )
            .build(k)
            .key("sessions-heading"),
        )
        .child(
            row()
                .key("session-search-field")
                .w_full()
                .h(32.0)
                .px(10.0)
                .gap(7.0)
                .center_y()
                .radius(6.0)
                .fill(k.c.bg_base)
                .border(
                    1.0,
                    if l.panels.sessions.focused {
                        k.c.border_focus
                    } else {
                        k.c.border_muted
                    },
                )
                .child(
                    icon(&glyphs::SEARCH, 13.0)
                        .color(k.c.icon_muted)
                        .a11y_hidden(),
                )
                .child(
                    text_input(&l.panels.sessions, k.ty.ui)
                        .single_line()
                        .label("Search sessions")
                        .placeholder("Search sessions", k.c.text_muted)
                        .color(k.c.text_base)
                        .selection_color(k.selection())
                        .grow(1.0)
                        .min_w(0.0)
                        .h(30.0)
                        .py(5.0)
                        .edit_menu("session-search", |s| Some(&mut live_mut(s).panels.sessions))
                        .on_input_key(on_key),
                ),
        )
        .child(
            col()
                .key("sidebar-scroll")
                .w_full()
                .grow(1.0)
                .min_h(80.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(sessions)
                .scroll_bar(k),
        )
}

pub(super) fn overlays(_s: &Workspace, _k: &Kit, _layers: &mut Vec<El>) {}

pub(crate) fn select(s: &mut Workspace, index: usize) {
    let l = live_mut(s);
    let trace = l.latency.begin("navigate");
    l.latency.track(trace, Some(index), true);
    let group = l
        .app
        .group_path(&l.app.sessions[index].scope.project)
        .to_owned();
    l.app.select(index);
    l.app
        .preferences
        .worktree_expanded
        .entry(group)
        .or_insert(false);
    l.panels.screen = crate::panels::Screen::Conversation;
    l.menu = None;
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    let l = live(s);
    if !l.panels.sessions.text.is_empty() && ev.key == Key::Escape {
        live_mut(s).panels.sessions.clear();
        return true;
    }
    false
}

#[cfg(test)]
mod tests;
