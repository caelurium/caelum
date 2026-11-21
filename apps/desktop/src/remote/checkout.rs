use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::VmLink;
use rune::{El, Tx};
use rune_kit::Kit;
use std::time::Instant;

pub(crate) fn enter(s: &mut Workspace) {
    show(s, true);
}

pub(crate) fn open_session_row(s: &mut Workspace, link: &VmLink) {
    if live_mut(s).app.open_vm_thread(link) {
        // The Sessions list keeps keyboard focus so the next arrow selects the next row.
        show(s, false);
    }
}

fn show(s: &mut Workspace, focus_draft: bool) {
    let l = live_mut(s);
    l.panels.overlay = None;
    l.panels.screen = Screen::Remote;
    l.session_menu = None;
    l.remote.inspection = None;
    super::conversation::sync_editor(l);
    super::conversation::set_focus(l, focus_draft);
    l.schedule();
    s.inspector.set_open(false);
}

pub(crate) fn enter_new(s: &mut Workspace) {
    live_mut(s).reveal_new_session();
    enter(s);
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
    changed |= l.app.dismiss_other_vm_tool();
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
