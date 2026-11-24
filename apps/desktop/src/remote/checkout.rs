use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{VmLink, native::SessionIntent};
use rune::{El, Tx};
use rune_kit::Kit;
use std::time::Instant;

#[derive(Clone, Copy)]
pub(super) enum Open {
    Native(SessionIntent),
    Pi,
}

/// Home's chosen worktree can differ from the conversation behind it.
pub(super) fn launch_source(l: &Live) -> Option<&str> {
    let project = match l.panels.screen {
        Screen::Home => l.app.home.draft.project.as_deref()?,
        Screen::Remote => {
            let id = l.app.remote.saved.selected.as_ref()?;
            return Some(&l.app.remote.saved.checkouts.get(id)?.source.project);
        }
        _ => l.app.active_project.as_deref()?,
    };
    Some(
        l.app
            .native_checkout(project)
            .map_or(project, |c| &c.source.project),
    )
}

fn dismiss_wait(l: &mut Live) {
    if let Some((id, _)) = l.remote.open_when_ready.take() {
        l.app.dismiss_vm_destination(&id);
    }
}

pub(crate) fn launch_source_for<'a>(l: &'a Live, project: &'a str) -> &'a str {
    launch_source(l)
        .filter(|p| l.app.group_path(p) == l.app.group_path(project))
        .unwrap_or(project)
}

pub(crate) fn enter(s: &mut Workspace) {
    dismiss_wait(live_mut(s));
    if live(s).app.selected.is_some() && super::native::selected(live(s)).is_some() {
        let l = live_mut(s);
        l.panels.screen = Screen::Conversation;
        l.panels.set_overlay(None);
        l.sync_editors();
        l.focus();
        return;
    }
    if let Some(id) = live(s).app.remote.saved.selected.clone()
        && live(s).app.remote.saved.checkouts[&id]
            .thread
            .awaiting_native
    {
        session(s, &id, SessionIntent::Reopen);
        return;
    }
    show(s, true);
}

pub(crate) fn open_session_row(s: &mut Workspace, link: &VmLink) {
    dismiss_wait(live_mut(s));
    if live_mut(s).app.open_vm_thread(link) {
        if live(s).app.remote.saved.checkouts[&link.checkout]
            .thread
            .awaiting_native
        {
            session(s, &link.checkout, SessionIntent::Reopen);
            return;
        }
        // The Sessions list keeps keyboard focus so the next arrow selects the next row.
        show(s, false);
    }
}

fn show(s: &mut Workspace, focus_draft: bool) {
    let l = live_mut(s);
    l.panels.set_overlay(None);
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
    live_mut(s).app.dismiss_vm_destination(id);
    session(s, id, SessionIntent::Reopen);
}

pub(super) fn session(s: &mut Workspace, id: &str, intent: SessionIntent) {
    if super::native::open(live_mut(s), id, intent) {
        return;
    }
    if live(s).app.remote.saved.selected.as_deref() != Some(id) {
        live_mut(s).app.select_remote(id);
    }
    show(s, true);
    live_mut(s).remote.open_when_ready = Some((id.into(), Open::Native(intent)));
}

pub(super) fn launch_pi(s: &mut Workspace, id: &str) {
    let l = live_mut(s);
    if !l.app.can_start_vm_session(id) {
        return;
    }
    let current = if l.panels.screen == Screen::Remote {
        l.app.remote.saved.selected.as_deref()
    } else if l.panels.screen == Screen::Home {
        None
    } else {
        super::native::selected(l)
    };
    if current != Some(id) {
        let source = &l.app.remote.saved.checkouts[id].source.project;
        let project = launch_source_for(l, source).to_owned();
        l.app.check_vm_destination(id, project);
        l.remote.open_when_ready = Some((id.into(), Open::Pi));
        show(s, true);
    } else if l
        .app
        .new_vm_session(id, caelum_core::remote::VmProvider::Pi)
    {
        enter_new(s);
    }
}

pub(crate) fn manage(s: &mut Workspace, id: &str) {
    live_mut(s).app.dismiss_vm_destination(id);
    dismiss_wait(live_mut(s));
    live_mut(s).app.select_remote(id);
    show(s, true);
}

pub(crate) fn sync(l: &mut Live, tx: &mut Tx) -> bool {
    let mut changed = false;
    if l.panels.overlay() == Some(Screen::VmStats) && super::popup::owner(l).is_none() {
        l.panels.set_overlay(None);
        changed = true;
    }
    if l.panels.overlay() != Some(Screen::VmStats) {
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
            let thread = &l.app.remote.saved.checkouts[&id].thread;
            if (thread.is_blank() && thread.provider == caelum_core::remote::VmProvider::Codex)
                || thread.awaiting_native
            {
                l.remote.open_when_ready = Some((id, Open::Native(SessionIntent::Reopen)));
            }
            changed = true;
        }
    }
    if l.panels.screen != Screen::Remote && l.remote.restored && l.remote.open_when_ready.is_none()
    {
        l.app.remote.saved.selected = None;
    }
    if let Some((id, intent)) = l.remote.open_when_ready.clone() {
        if l.app.remote.saved.selected.as_deref() != Some(&id) {
            l.app.dismiss_vm_destination(&id);
            l.remote.open_when_ready = None;
        } else if l.panels.overlay().is_none()
            && !l.app.remote.jobs.contains_key(&id)
            && match intent {
                Open::Native(intent) => super::native::open(l, &id, intent),
                Open::Pi => {
                    l.app.can_open_vm_session(&id)
                        && l.app
                            .new_vm_session(&id, caelum_core::remote::VmProvider::Pi)
                }
            }
        {
            l.remote.open_when_ready = None;
            if matches!(intent, Open::Pi) {
                l.reveal_new_session();
                super::conversation::sync_editor(l);
                super::conversation::focus(l);
            }
            changed = true;
        }
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
