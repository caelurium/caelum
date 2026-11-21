//! Normal session slots share the desktop; only their checkout-owned commands cross SSH.
mod connections;
use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::{
    application::Provider,
    remote::{
        Job,
        native::{SessionIntent, Status, checkout_id},
    },
};
pub(crate) use connections::Connections;
use rune::{El, icon, row, text};
use rune_kit::{ButtonVariant, Kit};

enum Blocked {
    Removed,
    Account,
    Stopped,
}

impl Blocked {
    fn reason(&self) -> &'static str {
        match self {
            Self::Removed => "VM checkout removed.",
            Self::Account => "Connect the Boat account that owns this session in Settings.",
            Self::Stopped => "VM stopped. Your draft is saved.",
        }
    }
}

fn unavailable(app: &caelum_core::application::Workspace, id: &str) -> Option<Blocked> {
    let Some(c) = app.remote.saved.checkouts.get(id) else {
        return Some(Blocked::Removed);
    };
    if matches!(
        app.remote.connection,
        caelum_core::remote::Connection::SignedOut
    ) || app
        .remote
        .inventory
        .as_ref()
        .is_some_and(|i| i.account != c.source.account)
    {
        return Some(Blocked::Account);
    }
    if c.machine()
        .is_some_and(|m| matches!(m.state.as_str(), "stopped" | "archived" | "archiving"))
    {
        return Some(Blocked::Stopped);
    }
    None
}

pub(crate) fn selected(l: &Live) -> Option<&str> {
    checkout_id(l.app.active_project.as_deref()?)
}

pub(crate) fn ready(l: &Live) -> bool {
    let project = if l.panels.screen == Screen::Home {
        l.app.home.draft.project.as_deref()
    } else {
        if l.app
            .selected
            .is_some_and(|i| !l.app.sessions[i].scope.provider_available())
        {
            return false;
        }
        l.app.active_project.as_deref()
    };
    project.and_then(checkout_id).is_none_or(|id| {
        unavailable(&l.app, id).is_none()
            && !matches!(l.app.remote.jobs.get(id), Some(Job::Deleting(_)))
            && l.native.status(id) == Some(&Status::Connected)
    })
}

pub(crate) fn open(l: &mut Live, id: &str, intent: SessionIntent) -> bool {
    if l.app.open_vm_session(id, intent).is_none() {
        return false;
    }
    if l.fixture && l.native.status(id).is_none() {
        l.native.fixture(
            id,
            l.app.remote.saved.checkouts[id].native_target().unwrap(),
            false,
        );
    }
    l.reveal_new_session();
    l.panels.screen = Screen::Conversation;
    l.panels.overlay = None;
    l.panels.archived = false;
    l.session_menu = None;
    l.sync_editors();
    l.focus();
    l.schedule();
    true
}

pub(crate) fn poll(l: &mut Live) -> bool {
    if let Some(id) = selected(l).map(str::to_owned)
        && l.native.status(&id).is_none()
        && l.app.can_open_vm_session(&id)
        && unavailable(&l.app, &id).is_none()
        && let Some(target) = l
            .app
            .remote
            .saved
            .checkouts
            .get(&id)
            .and_then(|c| c.native_target())
        && let Some(wake) = l.waker()
    {
        l.native.ensure(&id, target, wake);
    }
    let mut changed = false;
    for (id, notice) in l.native.drain(&l.app) {
        let project = caelum_core::remote::native::project_key(&id);
        match notice {
            caelum_core::remote::native::Notice::Event(mut e) => {
                l.app.merge_broker_event(Some(&project), &mut e);
                changed |= l.apply_events([*e]);
            }
            caelum_core::remote::native::Notice::Status(status) => {
                match status {
                    Status::Connected => l.app.native_connected(&project),
                    Status::Disconnected(_) => {
                        let retired = l.app.native_connection_lost(&project);
                        l.apply_events(retired);
                        if unavailable(&l.app, &id).is_none()
                            && !l.app.remote.jobs.contains_key(&id)
                            && l.app.can_open_vm_session(&id)
                        {
                            l.app.refresh_remote(&id);
                        }
                    }
                    Status::Connecting => {}
                }
                changed = true;
            }
        }
    }
    changed
}

pub(crate) fn banner(s: &Workspace, k: &Kit) -> Option<El> {
    let l = live(s);
    if l.panels.screen != Screen::Conversation {
        return None;
    }
    let id = selected(l)?;
    let checkout = l.app.remote.saved.checkouts.get(id)?;
    let job = l.app.remote.jobs.get(id);
    enum Action {
        Resume,
        Refresh,
        Reconnect,
        NewCodex,
    }
    let stopped = checkout
        .machine()
        .is_some_and(|m| matches!(m.state.as_str(), "stopped" | "archived"));
    let (message, action) = match (unavailable(&l.app, id), job, l.native.status(id)) {
        (_, Some(Job::Deleting(_)), _) => ("Deleting VM on Boat…", None),
        (Some(block), _, _) => (
            block.reason(),
            (stopped && matches!(block, Blocked::Stopped)).then_some(Action::Resume),
        ),
        _ if l
            .app
            .selected
            .is_some_and(|i| !l.app.sessions[i].scope.provider_available()) =>
        {
            (
                "OpenCode is unavailable for native VM sessions. Your draft is saved.",
                l.app.can_open_vm_session(id).then_some(Action::NewCodex),
            )
        }
        (_, _, Some(Status::Connected)) => return None,
        (_, _, Some(Status::Disconnected(e))) => (e.as_str(), Some(Action::Reconnect)),
        (_, Some(Job::Failed(e)), _) => (e.as_str(), Some(Action::Refresh)),
        _ if !l.app.can_open_vm_session(id) => (
            "Preparing VM checkout…",
            job.is_none().then_some(Action::Refresh),
        ),
        _ => ("Connecting to VM…", None),
    };
    let title = format!("{} · {message}", checkout.title());
    let id = id.to_owned();
    Some(
        row()
            .w_full()
            .min_h(30.)
            .px(12.)
            .gap(6.)
            .center_y()
            .key("native-workspace-status")
            .role("status")
            .label(&title)
            .child(icon(&ui::SERVER, 13.).color(k.c.icon_muted))
            .child(
                text(title)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .grow(1.)
                    .min_w(0.)
                    .wrap(),
            )
            .children(action.map(|action| {
                let (label, key) = match action {
                    Action::Resume => ("Resume VM", "resume-inline-vm"),
                    Action::Refresh => ("Refresh checkout", "refresh-inline-vm"),
                    Action::Reconnect => ("Reconnect", "native-workspace-reconnect"),
                    Action::NewCodex => ("New Codex session", "native-new-codex"),
                };
                ui::Button::new(label)
                    .variant(ButtonVariant::GhostMuted)
                    .disabled(matches!(action, Action::Resume) && matches!(job, Some(Job::Busy(_))))
                    .on_press(move |s: &mut Workspace| match action {
                        Action::Reconnect => live_mut(s).native.retry(&id),
                        Action::Refresh => live_mut(s).app.refresh_remote(&id),
                        Action::Resume => live_mut(s).app.power_remote(&id, true),
                        Action::NewCodex => super::new_session(s, &id),
                    })
                    .build(k)
                    .key(key)
            })),
    )
}

pub(super) fn scene(s: &mut Workspace, offline: bool) {
    super::scene::scene(s, "remote");
    let l = live_mut(s);
    let id = "checkout-example";
    open(l, id, SessionIntent::New);
    let i = l.app.selected.unwrap();
    l.app.sessions[i].remote_id = Some("codex-thread-example".into());
    l.app.sessions[i].title = "Verify the VM checkout is on the published worktree branch before running the integration tests".into();
    l.app.sessions[i].draft = "Check the tests for this change.".into();
    l.drafts[i].replace_text(&l.app.sessions[i].draft);
    l.app.sessions[i].conversation.history(Provider::Codex, &[serde_json::json!({
        "id":"turn-example","status":"completed","items":[
            {"type":"userMessage","id":"user-example","content":[{"type":"text","text":"Which branch is checked out on the VM?"}]},
            {"type":"agentMessage","id":"assistant-example","text":"The VM is on `feat/remote-checkout`. The changes are in `src/server.rs`."}
        ]
    })],false);
    l.native.fixture(
        id,
        l.app.remote.saved.checkouts[id].native_target().unwrap(),
        offline,
    );
    if offline {
        let events = l
            .app
            .native_connection_lost(&l.app.sessions[i].scope.project.clone());
        l.apply_events(events);
    }
}
