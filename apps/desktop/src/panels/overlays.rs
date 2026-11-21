use rune::{El, col};
use rune_kit::{Dialog, Kit};

use crate::Workspace;
use crate::live::{live, live_mut};

use super::Screen;
use super::launcher::launcher;

pub(super) fn close(s: &mut Workspace) {
    let l = live_mut(s);
    match l.panels.overlay.take() {
        Some(Screen::Fork) => l.app.dismiss_fork(),
        Some(Screen::VmHandoff | Screen::VmStats) => crate::remote::close_popup(l),
        Some(Screen::RemoveVm) => l.remote.removing = None,
        _ => {}
    }
    l.focus();
}

pub fn overlays(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(screen) = live(s).panels.overlay else {
        return;
    };
    if matches!(screen, Screen::VmHandoff | Screen::VmStats) {
        crate::remote::popup(s, k, layers);
        return;
    }
    let mut menus = Vec::new();
    let (title, description, body) = match screen {
        Screen::Settings => ("Settings", "", crate::settings::view(s, k, &mut menus)),
        Screen::CreateVm => (
            "Create VM",
            "Start a Boat checkout from a published GitHub branch.",
            crate::remote::create_view(s, k),
        ),
        Screen::RemotePicker => (
            "Open on remote machine",
            "Use an existing Boat checkout or create a new one.",
            crate::remote::picker(s, k),
        ),
        Screen::RemoveVm => (
            if live(s)
                .remote
                .removing
                .as_ref()
                .is_some_and(|(_, k)| k.is_some())
            {
                "Delete VM on Boat?"
            } else {
                "Remove VM setup?"
            },
            "",
            crate::remote::removal(s, k),
        ),
        Screen::VmThreads => (
            "Conversations on this VM",
            "Each conversation has its own chat and draft.",
            crate::remote::threads_view(s, k),
        ),
        Screen::Worktrees => (
            "Caelum worktrees",
            "Created in Caelum for this project.",
            crate::checkouts::manager(s, k),
        ),
        Screen::CreateWorktree => (
            "Create worktree",
            "A fresh checkout for an independent session.",
            crate::worktree::view(s, k, &mut menus),
        ),
        Screen::Shells => (
            "Background commands",
            "Live output from this session.",
            crate::shells::panel(s, k),
        ),
        Screen::Timeline => (
            "Timeline",
            "Find and reveal an earlier prompt.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Fork => {
            let (title, description) = crate::commands::history_header(s);
            (
                title,
                description,
                crate::commands::view(s, k, screen, &mut menus),
            )
        }
        Screen::Rename => (
            "Rename session",
            "This title is saved with the provider and stops automatic naming in Caelum.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Sessions => (
            "Sessions",
            "Browse earlier conversations from this provider.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Usage => (
            "OpenCode Go usage",
            "Subscription allowances.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Stats => (
            "Statistics",
            "Tokens, cost and activity across recorded history.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Image => (
            "Image preview",
            "Fit, zoom and scroll the image.",
            crate::media::view(s, k),
        ),
        Screen::Attachment => {
            let (title, description) = crate::composer::preview_header(s);
            (title, description, crate::composer::preview(s, k))
        }
        Screen::Launcher => (
            "New session",
            "Choose a local checkout or run Codex on a VM.",
            launcher(s, k),
        ),
        _ => return,
    };
    let (width, height): (f32, f32) = match screen {
        Screen::Settings => (740.0, crate::settings::height(s)),
        Screen::Launcher => (560.0, 380.0),
        Screen::Worktrees => (560.0, crate::checkouts::manager_height(s)),
        Screen::RemoveVm => (
            460.0,
            if live(s)
                .remote
                .removing
                .as_ref()
                .is_some_and(|(id, _)| live(s).app.remote.delete_errors.contains_key(id))
            {
                292.0
            } else {
                242.0
            },
        ),
        Screen::CreateWorktree => (500.0, 330.0),
        Screen::CreateVm | Screen::RemotePicker => (556.0, crate::remote::create_height(s)),
        Screen::Timeline => (580.0, 344.0),
        Screen::Sessions | Screen::Fork => (580.0, 420.0),
        Screen::VmThreads => {
            let saved = &live(s).app.remote.saved;
            let parked = saved
                .selected
                .as_ref()
                .and_then(|id| saved.checkouts.get(id))
                .map_or(0, |c| c.other_threads.len());
            (460.0, 100.0 + 48.0 * parked.min(6) as f32)
        }
        Screen::Rename => (460.0, 155.0),
        Screen::Stats => (720.0, 620.0),
        Screen::Image => (760.0, 480.0),
        Screen::Attachment => (760.0, crate::composer::preview_height(s)),
        _ => (660.0, 420.0),
    };
    let available =
        rune::memo::window_layout("shell").map_or(height, |r| (r[3] - 170.0).max(220.0));
    let width = if matches!(
        screen,
        Screen::CreateVm | Screen::RemotePicker | Screen::VmThreads
    ) {
        rune::memo::window_layout("shell").map_or(width, |r| width.min((r[2] - 24.).max(1.0)))
    } else {
        width
    };
    let dialog = Dialog::new("workspace", title);
    let dialog = if description.is_empty() {
        dialog
    } else {
        dialog.description(description)
    };
    dialog
        .open(true)
        .click_outside(false)
        .width(width)
        .fit()
        .on_close(close)
        .child(
            col()
                .w_full()
                .h(height.min(available))
                .min_h(0.0)
                .px(if screen == Screen::Attachment {
                    16.0
                } else {
                    24.0
                })
                .pb(if screen == Screen::Attachment {
                    16.0
                } else {
                    24.0
                })
                .child(body),
        )
        .build(k, layers);
    layers.extend(menus);
}
