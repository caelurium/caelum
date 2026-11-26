use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::{
    application::HandoffSource,
    remote::{Job, Monitor, VmLink},
};
use rune::{El, ScrollSpec, col, row};
use rune_kit::{ButtonVariant, Kit, Placement, ScrollBar, popover};

pub(super) enum Inspection {
    Local {
        index: usize,
        link: VmLink,
        home: Option<u64>,
    },
    Remote(String),
}

pub(super) fn owner(l: &Live) -> Option<&str> {
    if l.panels.overlay() != Some(Screen::VmStats) {
        return None;
    }
    match l.remote.inspection.as_ref()? {
        Inspection::Local { index, link, home }
            if l.app.linked_vm(*index) == Some(link)
                && match home {
                    Some(g) => l.panels.screen == Screen::Home && *g == l.app.home.generation(),
                    None => {
                        l.panels.screen == Screen::Conversation && l.app.selected == Some(*index)
                    }
                } =>
        {
            Some(&link.checkout)
        }
        Inspection::Remote(id)
            if (l.panels.screen == Screen::Remote
                && l.app.remote.saved.selected.as_ref() == Some(id))
                || (l.panels.screen == Screen::Conversation
                    && super::native::selected(l) == Some(id)) =>
        {
            Some(id)
        }
        _ => None,
    }
}

pub(super) fn inspect_local(l: &mut Live, index: usize) -> bool {
    let Some(link) = l.app.linked_vm(index).cloned() else {
        return false;
    };
    l.app.dismiss_vm_handoff();
    l.remote.inspection = Some(Inspection::Local {
        index,
        link,
        home: (l.panels.screen == Screen::Home).then(|| l.app.home.generation()),
    });
    l.panels.set_overlay(Some(Screen::VmStats));
    l.schedule();
    true
}

pub(super) fn inspect_remote(l: &mut Live, id: &str) {
    l.remote.inspection = Some(Inspection::Remote(id.into()));
    l.panels.set_overlay(Some(Screen::VmStats));
    l.schedule();
}

pub(crate) fn clear(l: &mut Live) {
    l.app.dismiss_vm_handoff();
    l.remote.inspection = None;
    l.app.remote.monitor = Monitor::Off;
}

fn close(s: &mut Workspace) {
    let l = live_mut(s);
    clear(l);
    l.panels.set_overlay(None);
    l.focus();
}

fn machine(s: &Workspace, k: &Kit, id: &str) -> El {
    let l = live(s);
    let c = &l.app.remote.saved.checkouts[id];
    let mut body = col().w_full().gap(8.0);
    if let Some(Inspection::Local { index, home, .. }) = l.remote.inspection.as_ref() {
        let index = *index;
        let source = if home.is_some() {
            HandoffSource::Recent(index)
        } else {
            HandoffSource::Composer(index)
        };
        body = body
            .child(
                row()
                    .flex_wrap()
                    .gap(4.0)
                    .child(
                        ui::Button::new("Open VM conversation")
                            .disabled(!l.app.can_open_linked_vm(index))
                            .on_press(move |s: &mut Workspace| {
                                if owner(live(s)).is_some() && live_mut(s).app.open_linked_vm(index)
                                {
                                    super::checkout::enter(s);
                                }
                            })
                            .build(k)
                            .key("open-linked-vm"),
                    )
                    .child(
                        ui::Button::new("Choose another VM…")
                            .variant(ButtonVariant::GhostMuted)
                            .disabled(l.app.handoff_blocker(source).is_some())
                            .on_press(move |s: &mut Workspace| super::handoff::begin(s, source))
                            .build(k),
                    ),
            )
            .child(ui::divider(k));
    }
    if c.machine().is_some() {
        body = body.child(super::stats::details(s, k, id, c));
    } else {
        body = body.child(super::status::status(&l.app.remote, id, c).view(k));
    }
    if let Some(Job::Failed(error)) = l.app.remote.jobs.get(id) {
        body = body.child(ui::caption(error, k).wrap().color(k.c.danger.fg));
    }
    col()
        .w_full()
        .min_h(0.0)
        .grow(1.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
        .scroll_bar(k)
}

pub(crate) fn view(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let l = live(s);
    let (title, body, height) = if l.panels.overlay() == Some(Screen::VmStats) {
        let Some(id) = owner(l) else {
            return;
        };
        ("Machine stats", machine(s, k, id), 404.0)
    } else {
        (
            "Continue on VM",
            super::handoff::view(s, k),
            super::picker::handoff_height(s),
        )
    };
    let anchor = if l.panels.screen == Screen::Remote {
        "remote-badge"
    } else {
        "continue-on-vm"
    };
    let window = rune::memo::window_layout("shell").unwrap_or([0., 0., 800., 620.]);
    let content = col()
        .key("vm-popover")
        .label(title)
        .role("dialog")
        .modal(true, close)
        .focus_return(anchor)
        .focus_group()
        .w((window[2] - 16.0).clamp(1.0, 420.0))
        .h(height.min((window[3] - 16.0).max(1.0)))
        .min_h(0.0)
        .padding(10.0)
        .gap(8.0)
        .child(ui::popup_header(title, None, None, k))
        .child(body.px(4.0).min_h(0.0).grow(1.0));
    layers.push(ui::popup_scrim("vm-scrim", close));
    layers.push(popover::anchored(
        k,
        popover::raised(k, content, ui::POPUP_RADIUS),
        anchor,
        Placement::TopEnd,
        ui::POPUP_OFFSET,
    ));
}
