//! Workspace Overview dashboard: summary cards, grouped sessions, pull requests and worktrees.
mod cards;
mod pull_requests;
mod sessions;
mod usage;
mod view;
mod worktrees;

pub use view::view;

use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    time::{Duration, Instant},
};

use caelum_core::application::{FeedSession, Provider, SessionBucket, Task};
use rune::{Color, El, Icon, Key, KeyEvent, LoopSpec, TextEdit, Tx, el, icon};

use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
};

pub(super) static WAITING: Icon = Icon {
    name: "overview-waiting",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="M8 5v3.5" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><circle cx="8" cy="11.1" r="0.8" fill="currentColor"/>"#,
};

static SPINNER_RING: Icon = Icon {
    name: "overview-spinner-ring",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5"/>"#,
};

static SPINNER_ARC: Icon = Icon {
    name: "overview-spinner-arc",
    view: 16.0,
    body: r#"<path d="M8 2A6 6 0 0 1 13.258 10.89" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>"#,
};

pub(super) fn running_spinner(size: f32, tone: Color, animated: bool) -> El {
    let arc = icon(&SPINNER_ARC, size)
        .key(if animated {
            "overview-spinner-arc"
        } else {
            "overview-spinner-arc-idle"
        })
        .absolute(0.0, 0.0)
        .color(tone);
    let arc = if animated {
        arc.looping(LoopSpec::rotation(0.9))
    } else {
        arc
    };
    el().size(size, size)
        .a11y_hidden()
        .child(
            icon(&SPINNER_RING, size)
                .key("overview-spinner-ring")
                .absolute(0.0, 0.0)
                .color(tone)
                .opacity(0.22),
        )
        .child(arc)
}

pub(super) static SETTLED: Icon = Icon {
    name: "overview-settled",
    view: 16.0,
    body: r#"<circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3"/><path d="m5.5 8.1 1.7 1.7 3.3-3.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) static LAPTOP: Icon = Icon {
    name: "overview-laptop",
    view: 16.0,
    body: r#"<rect x="3" y="3.5" width="10" height="7" rx="1.2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M1.5 12.5h13" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>"#,
};

pub(super) static PULL_REQUEST: Icon = Icon {
    name: "overview-pr",
    view: 16.0,
    body: r#"<circle cx="4.5" cy="4" r="1.6" fill="none" stroke="currentColor" stroke-width="1.2"/><circle cx="4.5" cy="12" r="1.6" fill="none" stroke="currentColor" stroke-width="1.2"/><circle cx="11.5" cy="12" r="1.6" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M4.5 5.6v4.8M9 4h1.3A1.2 1.2 0 0 1 11.5 5.2v5.2M9.8 2.5 8.2 4l1.6 1.5" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

pub(super) static RESET: Icon = Icon {
    name: "overview-reset",
    view: 16.0,
    body: r#"<path d="M13 8a5 5 0 1 1-1.3-3.4M13 3.2v2.6h-2.6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Sessions,
    PullRequests,
    Worktrees,
}

#[derive(Default)]
pub(super) struct Frame {
    turns: HashSet<usize>,
    next_age: Option<u64>,
}

impl Frame {
    pub(super) fn reset(&mut self) {
        self.turns.clear();
        self.next_age = None;
    }

    pub(super) fn age(&mut self, start: u64, now: u64) -> Option<String> {
        if start == 0 {
            return None;
        }
        let elapsed = now.saturating_sub(start);
        let (unit, suffix) = if elapsed < 3_600_000 {
            (60_000, "m")
        } else {
            (3_600_000, "h")
        };
        let count = elapsed / unit;
        let next = start.saturating_add((count + 1).saturating_mul(unit));
        self.next_age = Some(self.next_age.map_or(next, |at| at.min(next)));
        (count > 0).then(|| format!("{count}{suffix}"))
    }
}

pub struct State {
    pub tab: Tab,
    pub search: TextEdit,
    pub archived: bool,
    pub cursor: Option<FeedSession>,
    pub collapsed: [bool; 3],
    return_to: Option<Screen>,
    pub(super) pending_bookmark: Cell<Option<usize>>,
    pub(super) captured_bookmark: RefCell<Option<crate::live::Bookmark>>,
    pub(super) frame: RefCell<Frame>,
    wake_at: Option<u64>,
    host_poll_at: Option<Instant>,
    usage_requested: bool,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tab: Tab::Sessions,
            search: TextEdit::new(""),
            archived: false,
            cursor: None,
            collapsed: [false; 3],
            return_to: None,
            pending_bookmark: Cell::new(None),
            captured_bookmark: RefCell::new(None),
            frame: RefCell::new(Frame::default()),
            wake_at: None,
            host_poll_at: None,
            usage_requested: false,
        }
    }
}

impl State {
    pub(crate) fn shows_turn(&self, index: usize) -> bool {
        self.frame.borrow().turns.contains(&index)
    }

    pub(super) fn watch_turn(&self, index: usize) {
        self.frame.borrow_mut().turns.insert(index);
    }

    pub(crate) fn tick(&mut self, now: u64, tx: &mut Tx) -> bool {
        let frame = self.frame.get_mut();
        let next = frame.next_age;
        if next.is_some_and(|at| now >= at) {
            frame.next_age = None;
            self.wake_at = None;
            return true;
        }
        if self.wake_at != next {
            self.wake_at = next;
            if let Some(at) = next {
                tx.wake_after(Duration::from_millis(at - now));
            }
        }
        false
    }

    fn bucket_index(bucket: SessionBucket) -> usize {
        match bucket {
            SessionBucket::Waiting => 0,
            SessionBucket::Running => 1,
            SessionBucket::Settled => 2,
        }
    }

    pub(super) fn is_collapsed(&self, bucket: SessionBucket) -> bool {
        self.collapsed[Self::bucket_index(bucket)]
    }

    pub(super) fn toggle_bucket(&mut self, bucket: SessionBucket) {
        let slot = &mut self.collapsed[Self::bucket_index(bucket)];
        *slot = !*slot;
    }

    pub(super) fn expand_bucket(&mut self, bucket: SessionBucket) {
        self.collapsed[Self::bucket_index(bucket)] = false;
    }
}

pub fn is_overview(s: &Workspace) -> bool {
    s.live
        .as_ref()
        .is_some_and(|l| l.panels.screen == Screen::Overview)
}

pub fn needs_bookmark(s: &Workspace) -> bool {
    s.live.as_ref().is_some_and(|l| {
        l.panels.screen == Screen::Overview && l.overview.pending_bookmark.get().is_some()
    })
}

pub(crate) fn shortcut(event: &KeyEvent) -> bool {
    event.is_command('j') && !event.mods.shift && !event.mods.alt
}

pub fn open(s: &mut Workspace, _tx: &mut Tx) {
    let l = live_mut(s);
    let in_secondary_split = l.split.is_visible(l.window_width.get())
        && l.split.active == crate::split::ActivePane::Secondary;
    if in_secondary_split {
        let orientation = l.split.orientation;
        l.split
            .push_pane(crate::split::SecondaryPane::Overview, orientation);
        l.overview.cursor = None;
        l.overview.search.clear();
        l.overview.search.focused = true;
        l.overview.usage_requested = false;
        l.panels.set_overlay(None);
        l.session_menu = None;
        l.models = Default::default();
        l.completion = Default::default();
        for draft in &mut l.drafts {
            draft.focused = false;
        }
        return;
    }
    if l.panels.screen == Screen::Overview {
        l.overview.search.focused = true;
        return;
    }
    l.panels.remember(&mut l.app);
    let scrolled_up = (l.panels.screen == Screen::Conversation)
        .then_some(l.app.selected)
        .flatten()
        .filter(|&owner| {
            let pos = l.reading_position(owner);
            pos.offset[1] < pos.max[1] - 1.0
        });
    l.overview.pending_bookmark.set(scrolled_up);
    *l.overview.captured_bookmark.get_mut() = None;
    l.overview.return_to = Some(l.panels.screen);
    l.overview.cursor = None;
    l.overview.search.clear();
    l.overview.search.focused = true;
    l.overview.usage_requested = false;
    l.panels.screen = Screen::Overview;
    l.panels.set_overlay(None);
    l.session_menu = None;
    l.models = Default::default();
    l.completion = Default::default();
    for draft in &mut l.drafts {
        draft.focused = false;
    }
    l.home.editor.focused = false;
    s.inspector.set_open(false);
}

pub fn open_from_nav(s: &mut Workspace) {
    open(s, &mut Tx::default());
}

pub(crate) fn close(s: &mut Workspace, tx: &mut Tx) {
    let l = live_mut(s);
    if l.split.visible_secondary() == Some(&crate::split::SecondaryPane::Overview)
        && l.split.active == crate::split::ActivePane::Secondary
    {
        l.overview.search.focused = false;
        l.overview.frame.get_mut().reset();
        crate::split::close(s);
        return;
    }
    if l.panels.screen != Screen::Overview {
        return;
    }
    if let Some(mark) = l.overview.captured_bookmark.get_mut().take() {
        l.park_reading(mark);
    }
    l.overview.pending_bookmark.set(None);
    l.overview.search.focused = false;
    l.overview.frame.get_mut().reset();
    let target = l.overview.return_to.take().unwrap_or_else(|| {
        if l.app.selected.is_some() {
            Screen::Conversation
        } else if l.app.remote.saved.selected.is_some() {
            Screen::Remote
        } else {
            Screen::Home
        }
    });
    l.panels.screen = match target {
        Screen::Conversation if l.app.selected.is_none() => Screen::Home,
        Screen::Remote if l.app.remote.saved.selected.is_none() => Screen::Home,
        other => other,
    };
    if l.panels.screen == Screen::Conversation
        && let Some(index) = l.app.selected
    {
        l.restore_reading(index, tx);
    }
    l.focus();
}

pub fn sync(l: &mut Live, tx: &mut Tx) -> bool {
    if let Some(mark) = l.overview.captured_bookmark.get_mut().take() {
        l.park_reading(mark);
        l.clear_visible_entries();
    }
    let shown = (l.panels.screen == Screen::Overview
        || l.split.visible_secondary() == Some(&crate::split::SecondaryPane::Overview))
        && l.panels.overlay().is_none();
    if !shown {
        l.overview.host_poll_at = None;
        l.overview.frame.get_mut().reset();
        return false;
    }
    let mut changed = false;
    if !l.fixture && !l.app.window_background {
        changed |= l.overview.tick(l.feed.displayed_at(), tx);
        let now = Instant::now();
        let should_poll_host = l
            .overview
            .host_poll_at
            .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(5));
        if should_poll_host && !l.app.services.busy(&Task::HostStats) {
            l.app.task(Task::HostStats);
            l.overview.host_poll_at = Some(now);
            changed = true;
        }
        tx.wake_after(Duration::from_secs(5));
        if !l.overview.usage_requested {
            l.overview.usage_requested = true;
            request_usage(l);
        }
    }
    changed
}

fn request_usage(l: &mut Live) {
    let Some(project) = l
        .app
        .active_project
        .clone()
        .or_else(|| l.app.projects.first().map(|p| p.path.clone()))
    else {
        return;
    };
    for provider in [Provider::Antigravity, Provider::OpenCode] {
        let has_session = l
            .app
            .sessions
            .iter()
            .any(|s| !s.forgotten() && s.scope.provider == provider);
        if !has_session && l.app.preferences.default_provider != provider {
            continue;
        }
        let task = Task::Usage {
            project: project.clone(),
            provider,
        };
        let cached = l
            .app
            .services
            .checkouts
            .get(&project)
            .is_some_and(|c| c.usage.contains_key(&provider));
        if !cached && !l.app.services.busy(&task) {
            l.app.task(task);
        }
    }
}

pub(super) fn refresh_all(s: &mut Workspace) {
    let l = live_mut(s);
    l.app.refresh_project();
    l.app.refresh_feed_pull_requests(None);
    l.app.discover_open_sessions();
    l.app.refresh_open_sessions();
    if !l.fixture {
        l.app.task(Task::HostStats);
        l.overview.host_poll_at = Some(Instant::now());
        if let Some(project) = l
            .app
            .active_project
            .clone()
            .or_else(|| l.app.projects.first().map(|p| p.path.clone()))
        {
            for provider in [Provider::Antigravity, Provider::OpenCode] {
                l.app.task(Task::Usage {
                    project: project.clone(),
                    provider,
                });
            }
        }
    }
}

pub(super) fn row_key(id: &FeedSession) -> String {
    match id {
        FeedSession::Local(i) => format!("overview-session-{i}"),
        FeedSession::Vm(link) => crate::code_view::source_key(&link.checkout, &link.thread),
    }
}

pub(super) fn choose(s: &mut Workspace, id: &FeedSession, tx: &mut Tx) {
    if let Some(mark) = live_mut(s).overview.captured_bookmark.get_mut().take() {
        live_mut(s).park_reading(mark);
    }
    live_mut(s).overview.pending_bookmark.set(None);
    let l = live(s);
    let current = l.app.selected_feed_session().as_ref() == Some(id);
    match id {
        FeedSession::Local(i)
            if l.app
                .sessions
                .get(*i)
                .is_some_and(|sess| !sess.archiving() && !sess.deleting()) =>
        {
            live_mut(s).overview.search.focused = false;
            live_mut(s).overview.return_to = None;
            crate::live::select_session(s, *i);
            if !current {
                live(s).restore_reading(*i, tx);
            }
        }
        FeedSession::Vm(link) if l.app.can_open_vm_thread(link) => {
            live_mut(s).overview.search.focused = false;
            live_mut(s).overview.return_to = None;
            crate::remote::open_session_row(s, link);
        }
        _ => return,
    }
    live_mut(s).focus();
}

pub(super) fn next_waiting(s: &mut Workspace, tx: &mut Tx) {
    let l = live_mut(s);
    l.overview.tab = Tab::Sessions;
    l.overview.expand_bucket(SessionBucket::Waiting);
    let rows = sessions::ordered_sessions(l);
    let start = l
        .overview
        .cursor
        .as_ref()
        .and_then(|id| rows.iter().position(|r| r == id))
        .map_or(0, |i| i + 1);
    if let Some(next) = (start..rows.len())
        .chain(0..start)
        .find(|&i| l.app.session_bucket(&rows[i]) == SessionBucket::Waiting)
    {
        let id = rows[next].clone();
        l.overview.cursor = Some(id.clone());
        l.overview.search.focused = true;
        tx.scroll_center_in_instant("overview-rows", row_key(&id));
    }
}

fn overview_active_pane(s: &Workspace) -> bool {
    let Some(l) = s.live.as_ref() else {
        return false;
    };
    if l.split.is_visible(l.window_width.get()) {
        match l.split.active {
            crate::split::ActivePane::Primary => l.panels.screen == Screen::Overview,
            crate::split::ActivePane::Secondary => {
                l.split.visible_secondary() == Some(&crate::split::SecondaryPane::Overview)
            }
        }
    } else {
        l.panels.screen == Screen::Overview
    }
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if shortcut(ev) {
        if !tx.key_repeat() {
            if overview_active_pane(s) {
                close(s, tx);
            } else {
                open(s, tx);
            }
        }
        return true;
    }
    if !overview_active_pane(s) {
        return false;
    }
    if ev.key == Key::Escape {
        if !tx.key_repeat() {
            close(s, tx);
        }
        return true;
    }
    if live(s).overview.tab != Tab::Sessions {
        return false;
    }
    if !matches!(ev.key, Key::Up | Key::Down | Key::Enter)
        || ev.mods.shift
        || ev.mods.alt
        || ev.mods.cmd
        || ev.mods.ctrl
    {
        return false;
    }
    if tx.focused() == Some("overview-requests") && ev.key == Key::Enter {
        next_waiting(s, tx);
        return true;
    }
    let l = live(s);
    let rows = sessions::ordered_sessions(l);
    if rows.is_empty() {
        return ev.key == Key::Enter;
    }
    let current = tx
        .focused()
        .filter(|k| *k != "overview-search")
        .and_then(|key| rows.iter().position(|row| row_key(row) == key))
        .or_else(|| {
            l.overview
                .cursor
                .as_ref()
                .and_then(|id| rows.iter().position(|r| r == id))
        })
        .unwrap_or(0);
    match ev.key {
        Key::Enter => {
            if !tx.key_repeat()
                && let Some(id) = rows.get(current)
            {
                choose(s, id, tx);
            }
            true
        }
        Key::Up => {
            let next = current.saturating_sub(1);
            let id = rows[next].clone();
            let l = live_mut(s);
            l.overview.cursor = Some(id.clone());
            l.overview.search.focused = true;
            tx.scroll_center_in_instant("overview-rows", row_key(&id));
            true
        }
        Key::Down => {
            let next = (current + 1).min(rows.len().saturating_sub(1));
            let id = rows[next].clone();
            let l = live_mut(s);
            l.overview.cursor = Some(id.clone());
            l.overview.search.focused = true;
            tx.scroll_center_in_instant("overview-rows", row_key(&id));
            true
        }
        _ => false,
    }
}
