mod card;
mod view;

pub(crate) use view::view;

use super::{Overlay, Panels};
use crate::{
    Workspace,
    live::{live, live_mut},
    quiet_button, ui,
};
use caelum_core::application::FeedSession;
use rune::{El, Icon, Key, KeyEvent, TextEdit, Tx};
use rune_kit::Kit;

const SPREAD: Icon = Icon {
    name: "conversation-spread",
    view: 20.0,
    body: r#"<g fill="none" stroke="currentColor" stroke-width="1.3"><rect x="2" y="3" width="7" height="9" rx="1.5"/><rect x="11.5" y="3" width="6.5" height="5.5" rx="1.5"/><rect x="11.5" y="11" width="6.5" height="6" rx="1.5"/><path d="M3 15.5h5"/></g>"#,
};

pub(crate) struct State {
    search: TextEdit,
    cursor: Option<FeedSession>,
    columns: usize,
    origin: std::cell::OnceCell<crate::live::Bookmark>,
}

impl Panels {
    pub(super) fn conversations(&self) -> Option<&State> {
        match &self.overlay {
            Some(Overlay::Conversations(state)) => Some(state),
            _ => None,
        }
    }

    fn conversations_mut(&mut self) -> Option<&mut State> {
        match &mut self.overlay {
            Some(Overlay::Conversations(state)) => Some(state),
            _ => None,
        }
    }
}

pub(crate) fn trigger(k: &Kit) -> El {
    quiet_button(&SPREAD, "Conversation overview · ⌘J")
        .on_press_tx(open)
        .build(k)
        .key("open-conversations")
        .shortcut_hint(ui::shortcut("⌘J"))
}

pub(crate) fn shortcut(event: &KeyEvent) -> bool {
    event.is_command('j') && !event.mods.shift && !event.mods.alt
}

pub(crate) fn open(s: &mut Workspace, _: &mut Tx) {
    let l = live_mut(s);
    if l.panels.overlay().is_some() {
        return;
    }
    let mut search = TextEdit::new("");
    search.focused = true;
    l.feed.close();
    l.session_menu = None;
    l.panels.overlay = Some(Overlay::Conversations(Box::new(State {
        search,
        cursor: l.app.selected_feed_session(),
        columns: 0,
        origin: Default::default(),
    })));
}

fn close(s: &mut Workspace) {
    live_mut(s).panels.set_overlay(None);
}

fn tile_key(id: &FeedSession) -> String {
    match id {
        FeedSession::Local(i) => format!("overview-session-{i}"),
        FeedSession::Vm(link) => crate::code_view::source_key(&link.checkout, &link.thread),
    }
}

fn choose(s: &mut Workspace, id: &FeedSession, tx: &mut Tx) {
    let l = live(s);
    let Some(state) = l.panels.conversations() else {
        return;
    };
    if !l.app.overview_sessions(&state.search.text).contains(id) {
        return;
    }
    let current = l.app.selected_feed_session().as_ref() == Some(id)
        && matches!(
            l.panels.screen,
            super::Screen::Conversation | super::Screen::Remote
        );
    match id {
        FeedSession::Local(i)
            if !l.app.sessions[*i].archiving() && !l.app.sessions[*i].deleting() =>
        {
            leave(s, current);
            crate::live::select_session(s, *i);
            if !current {
                live(s).restore_reading(*i, tx);
            }
        }
        FeedSession::Vm(link) if l.app.can_open_vm_thread(link) => {
            leave(s, current);
            crate::remote::open_session_row(s, link);
        }
        _ => return,
    }
    live_mut(s).focus();
}

fn leave(s: &mut Workspace, current: bool) {
    let l = live_mut(s);
    if let Some(Overlay::Conversations(state)) = l.panels.overlay.take()
        && !current
        && let Some(mark) = state.origin.into_inner()
    {
        l.park_reading(mark);
    }
}

fn reveal(tx: &mut Tx, index: usize, columns: usize) {
    tx.scroll_center_in_instant(
        "overview-rows",
        format!("overview-row-{}", index / columns.max(1)),
    );
}

fn needs_input(l: &crate::live::Live, id: &FeedSession) -> bool {
    matches!(id, FeedSession::Local(i) if l.app.needs_input(&l.app.sessions[*i]))
}

fn next_request(s: &mut Workspace, tx: &mut Tx) {
    let l = live(s);
    let Some(state) = l.panels.conversations() else {
        return;
    };
    let rows = l.app.overview_sessions(&state.search.text);
    let start = state
        .cursor
        .as_ref()
        .and_then(|id| rows.iter().position(|r| r == id))
        .map_or(0, |i| i + 1);
    if let Some(next) = (start..rows.len())
        .chain(0..start)
        .find(|&i| needs_input(l, &rows[i]))
    {
        highlight(s, tx, &rows, next);
    }
}

fn highlight(s: &mut Workspace, tx: &mut Tx, rows: &[FeedSession], next: usize) {
    if let Some(id) = rows.get(next) {
        let state = live_mut(s).panels.conversations_mut().unwrap();
        state.cursor = Some(id.clone());
        state.search.focused = true;
        reveal(tx, next, state.columns);
    }
}

pub(crate) fn key(s: &mut Workspace, event: &KeyEvent, tx: &mut Tx) -> bool {
    if event.key == Key::Escape || shortcut(event) {
        if !tx.key_repeat() {
            close(s);
        }
        return true;
    }
    if !matches!(
        event.key,
        Key::Left | Key::Right | Key::Up | Key::Down | Key::Enter
    ) || event.mods.shift
        || event.mods.alt
        || event.mods.cmd
        || event.mods.ctrl
    {
        return ['n', 'o', 'p', 'l', 'k', ',', 'i', '\\']
            .iter()
            .any(|c| event.is_command(*c));
    }
    let l = live(s);
    let Some(state) = l.panels.conversations() else {
        return false;
    };
    if tx.focused() == Some("overview-close") {
        if event.key == Key::Enter {
            close(s);
            return true;
        }
        return false;
    }
    if tx.focused() == Some("overview-requests") && event.key == Key::Enter {
        next_request(s, tx);
        return true;
    }
    if matches!(event.key, Key::Left | Key::Right)
        && state.search.focused
        && !state.search.text.is_empty()
    {
        return false;
    }
    let rows = l.app.overview_sessions(&state.search.text);
    let current = tx
        .focused()
        .filter(|k| *k != "overview-search")
        .and_then(|key| rows.iter().position(|row| tile_key(row) == key))
        .or_else(|| {
            state
                .cursor
                .as_ref()
                .and_then(|id| rows.iter().position(|r| r == id))
        })
        .unwrap_or(0);
    let columns = state.columns.max(1);
    let next = match event.key {
        Key::Enter => {
            if !tx.key_repeat()
                && let Some(id) = rows.get(current)
            {
                choose(s, id, tx);
            }
            return true;
        }
        Key::Left => current.saturating_sub(1),
        Key::Right => current + 1,
        Key::Up => current.saturating_sub(columns),
        Key::Down => current + columns,
        _ => unreachable!(),
    }
    .min(rows.len().saturating_sub(1));
    highlight(s, tx, &rows, next);
    true
}
