use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{Job, Phase};
use rune::{El, Tx, col};
use rune_kit::Kit;
use std::time::Instant;

pub(super) fn enter(s: &mut Workspace) {
    let l = live_mut(s);
    l.panels.overlay = None;
    l.panels.screen = Screen::Remote;
    l.session_menu = None;
    l.remote.details = false;
    super::conversation::sync_editor(l);
    l.schedule();
    s.inspector.set_open(false);
}

pub(crate) fn select(s: &mut Workspace, id: &str) {
    live_mut(s).app.select_remote(id);
    enter(s);
}

pub(crate) fn sync(l: &mut Live, tx: &mut Tx) -> bool {
    let mut changed = false;
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
    let now = Instant::now();
    super::stats::sync(l, tx, now);
    if let Some(at) = l.app.feed_deadline(now) {
        tx.wake_after(at.saturating_duration_since(now));
    }
    super::conversation::sync_editor(l);
    if let Some(at) = l.app.remote_deadline(now) {
        tx.wake_after(at.saturating_duration_since(now));
    }
    changed |= super::changes::sync(l);
    changed
}

pub(crate) fn screen(s: &Workspace, k: &Kit) -> El {
    let r = &live(s).app.remote;
    let mut body = col().w_full().h_full().min_h(0.0).padding(20.0).gap(10.0);
    let Some((id, c)) = r
        .saved
        .selected
        .as_ref()
        .and_then(|id| r.saved.checkouts.get_key_value(id))
    else {
        return body;
    };
    if let Phase::Ready { branch, .. } = &c.phase {
        return super::conversation::view(s, k, id, c, branch);
    }
    if let Phase::Prepare {
        machine,
        new_branch: None,
    } = &c.phase
        && matches!(machine.state.as_str(), "archived" | "stopped")
    {
        return super::conversation::view(s, k, id, c, "Stopped");
    }
    body = body.child(ui::section(
        &format!("Remote · {}", c.title()),
        &c.source.repository,
        k,
    ));
    let detail = match &c.phase {
        Phase::Create(_) => "Creating machine…",
        Phase::Prepare { machine, .. } => &machine.state,
        Phase::Ready { branch, .. } => branch,
    };
    body = body.child(ui::caption(detail, k));
    if let Some(Job::Failed(e)) = r.jobs.get(id) {
        body = body.child(ui::caption(e, k).wrap().color(k.c.danger.fg));
    }
    let busy = matches!(r.jobs.get(id), Some(Job::Busy(_) | Job::Wait(_)));
    let key = id.clone();
    body = body.child(
        ui::Button::new(if busy {
            "Checking machine…"
        } else {
            "Refresh checkout"
        })
        .disabled(busy)
        .on_press(move |s: &mut Workspace| live_mut(s).app.refresh_remote(&key))
        .build(k)
        .self_start(),
    );
    if let Some(m) = c.machine() {
        let resume = matches!(m.state.as_str(), "archived" | "stopped");
        let id = id.clone();
        body = body.child(
            ui::Button::new(if resume {
                "Resume machine"
            } else {
                "Stop machine"
            })
            .disabled(busy)
            .on_press(move |s: &mut Workspace| live_mut(s).app.power_remote(&id, resume))
            .build(k)
            .self_start(),
        );
    }
    body
}
