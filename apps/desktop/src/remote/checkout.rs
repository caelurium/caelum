use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use rune::{El, Tx};
use rune_kit::Kit;
use std::time::Instant;

pub(crate) fn enter(s: &mut Workspace) {
    let l = live_mut(s);
    l.panels.overlay = None;
    l.panels.screen = Screen::Remote;
    l.session_menu = None;
    l.remote.inspection = None;
    super::conversation::sync_editor(l);
    super::conversation::focus(l);
    l.schedule();
    s.inspector.set_open(false);
}

pub(crate) fn select(s: &mut Workspace, id: &str) {
    live_mut(s).app.select_remote(id);
    enter(s);
}

pub(crate) fn sync(l: &mut Live, tx: &mut Tx) -> bool {
    let mut changed = false;
    if l.panels.overlay == Some(Screen::VmStats) && super::popup::owner(l).is_none() {
        l.panels.overlay = None;
        changed = true;
    }
    if l.panels.overlay != Some(Screen::VmStats) {
        l.remote.inspection = None;
    }
    if !l.remote.restored && l.app.restored() {
        l.remote.restored = true;
        if !l.app.opening
            && l.app.selected.is_none()
            && let Some(id) = l.app.remote.saved.selected.clone()
        {
            l.app.select_remote(&id);
            l.panels.screen = Screen::Remote;
            changed = true;
        }
    }
    if l.panels.screen != Screen::Remote && l.remote.restored {
        l.app.remote.saved.selected = None;
    }
    changed |= super::provider::sync(l);
    let now = Instant::now();
    super::stats::sync(l, tx, now);
    if let Some(at) = l.app.feed_deadline(now) {
        tx.wake_after(at.saturating_duration_since(now));
    }
    super::conversation::sync_editor(l);
    changed |= super::conversation::restore(l, tx);
    if let Some(at) = l.app.remote_deadline(now) {
        tx.wake_after(at.saturating_duration_since(now));
    }
    changed |= super::changes::sync(l);
    changed
}

pub(crate) fn screen(
    s: &Workspace,
    k: &Kit,
    padding: f32,
    reduced: bool,
    layers: &mut Vec<El>,
) -> El {
    let r = &live(s).app.remote;
    let Some((id, c)) = r
        .saved
        .selected
        .as_ref()
        .and_then(|id| r.saved.checkouts.get_key_value(id))
    else {
        return ui::empty("No remote checkout", "Choose a VM under its project.", k);
    };
    super::conversation::view(s, k, id, c, padding, reduced, layers)
}
