use rune::{El, col};
use rune_kit::{Dialog, Kit};

use crate::Workspace;
use crate::live::{live, live_mut};

use super::launcher::launcher;
use super::{Panels, Screen};

pub(super) enum Overlay {
    Standard(StandardOverlay),
    Attachment(Box<crate::composer::Preview>),
    CreateWorktree(Box<crate::worktree::Form>),
    Image(crate::media::Preview),
    Palette(Box<crate::palette::State>),
    RemoveVm(Removal),
    VmStats(crate::remote::Inspection),
}

macro_rules! standard_overlays {
    ($($name:ident),+ $(,)?) => {
        #[derive(Clone, Copy)]
        pub(super) enum StandardOverlay {
            $($name),+
        }

        impl StandardOverlay {
            fn from_screen(screen: Screen) -> Option<Self> {
                match screen {
                    $(Screen::$name => Some(Self::$name),)+
                    _ => None,
                }
            }

            fn screen(self) -> Screen {
                match self {
                    $(Self::$name => Screen::$name,)+
                }
            }
        }
    };
}

standard_overlays!(
    CreateVm,
    Fork,
    Launcher,
    RemotePicker,
    Rename,
    Runner,
    Session,
    Sessions,
    Settings,
    Shells,
    Stats,
    Timeline,
    Usage,
    VmHandoff,
    VmThreads,
    Worktrees,
);

pub(crate) struct Removal {
    pub checkout: String,
    pub machine: Option<caelum_core::remote::MachineId>,
}

impl Overlay {
    fn screen(&self) -> Screen {
        match self {
            Self::Standard(screen) => screen.screen(),
            Self::Attachment(_) => Screen::Attachment,
            Self::Image(_) => Screen::Image,
            Self::CreateWorktree(_) => Screen::CreateWorktree,
            Self::Palette(_) => Screen::Palette,
            Self::RemoveVm(_) => Screen::RemoveVm,
            Self::VmStats(_) => Screen::VmStats,
        }
    }
}

impl Panels {
    pub fn overlay(&self) -> Option<Screen> {
        self.overlay.as_ref().map(Overlay::screen)
    }

    pub fn set_overlay(&mut self, screen: Option<Screen>) {
        let overlay = screen
            .and_then(StandardOverlay::from_screen)
            .map(Overlay::Standard);
        if overlay.is_none()
            && let Some(Overlay::Attachment(preview)) = self.overlay.take()
        {
            self.attachment_restore = Some(preview.restore);
        }
        self.overlay = overlay;
    }

    pub(crate) fn open_attachment(&mut self, preview: crate::composer::Preview) {
        self.attachment_restore = None;
        self.overlay = Some(Overlay::Attachment(Box::new(preview)));
    }

    pub(crate) fn attachment(&self) -> Option<&crate::composer::Preview> {
        match &self.overlay {
            Some(Overlay::Attachment(preview)) => Some(preview),
            _ => None,
        }
    }

    pub(crate) fn take_attachment_restore(&mut self) -> Option<crate::composer::Restore> {
        self.attachment_restore.take()
    }

    pub(crate) fn open_image(
        &mut self,
        project: String,
        media: caelum_core::application::presentation::Media,
    ) {
        self.overlay = Some(Overlay::Image(crate::media::Preview::new(project, media)));
    }

    pub(crate) fn open_worktree(&mut self, form: crate::worktree::Form) {
        self.overlay = Some(Overlay::CreateWorktree(Box::new(form)));
    }

    pub(crate) fn open_vm_stats(&mut self, inspection: crate::remote::Inspection) {
        self.overlay = Some(Overlay::VmStats(inspection));
    }

    pub(crate) fn vm_stats(&self) -> Option<&crate::remote::Inspection> {
        match &self.overlay {
            Some(Overlay::VmStats(inspection)) => Some(inspection),
            _ => None,
        }
    }

    pub(crate) fn open_removal(&mut self, removal: Removal) {
        self.overlay = Some(Overlay::RemoveVm(removal));
    }

    pub(crate) fn worktree(&self) -> Option<&crate::worktree::Form> {
        match &self.overlay {
            Some(Overlay::CreateWorktree(form)) => Some(form),
            _ => None,
        }
    }

    pub(crate) fn worktree_mut(&mut self) -> Option<&mut crate::worktree::Form> {
        match &mut self.overlay {
            Some(Overlay::CreateWorktree(form)) => Some(form),
            _ => None,
        }
    }

    pub(crate) fn open_palette(&mut self, state: crate::palette::State) {
        self.overlay = Some(Overlay::Palette(Box::new(state)));
    }

    pub(crate) fn palette(&self) -> Option<&crate::palette::State> {
        match &self.overlay {
            Some(Overlay::Palette(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn palette_mut(&mut self) -> Option<&mut crate::palette::State> {
        match &mut self.overlay {
            Some(Overlay::Palette(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn removal(&self) -> Option<&Removal> {
        match &self.overlay {
            Some(Overlay::RemoveVm(removal)) => Some(removal),
            _ => None,
        }
    }
}

pub(super) fn close(s: &mut Workspace) {
    let l = live_mut(s);
    let overlay = l.panels.overlay();
    l.panels.set_overlay(None);
    match overlay {
        Some(Screen::Fork) => l.app.dismiss_fork(),
        Some(Screen::Usage) => l.commands.usage_target = None,
        Some(Screen::VmHandoff | Screen::VmStats) => crate::remote::close_popup(l),
        _ => {}
    }
    l.focus();
}

pub fn overlays(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(screen) = live(s).panels.overlay() else {
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
            "Choose a harness and review the branch for your Boat checkout.",
            crate::remote::create_view(s, k),
        ),
        Screen::RemotePicker => (
            "Open on remote machine",
            "Use an existing Boat checkout or create a new one.",
            crate::remote::picker(s, k),
        ),
        Screen::RemoveVm => (
            if live(s)
                .panels
                .removal()
                .is_some_and(|removal| removal.machine.is_some())
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
        Screen::Runner => (
            "Run checkout command",
            "Run a command in this checkout and attach its output to your prompt.",
            crate::runner::panel(s, k),
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
        Screen::Usage => {
            let antigravity = crate::commands::usage_provider(s)
                == caelum_core::application::Provider::Antigravity;
            (
                if antigravity {
                    "Antigravity usage"
                } else {
                    "OpenCode Go usage"
                },
                if antigravity {
                    "Quota and reset windows."
                } else {
                    "Subscription allowances."
                },
                crate::commands::view(s, k, screen, &mut menus),
            )
        }
        Screen::Stats => (
            "Statistics",
            "Tokens, cost and activity across recorded history.",
            crate::commands::view(s, k, screen, &mut menus),
        ),
        Screen::Image => {
            let Some(Overlay::Image(preview)) = &live(s).panels.overlay else {
                return;
            };
            (
                "Image preview",
                "Fit, zoom and scroll the image.",
                crate::media::view(
                    s,
                    k,
                    &preview.project,
                    &preview.media.name,
                    &preview.media.source,
                    &preview.viewport,
                ),
            )
        }
        Screen::Attachment => {
            let Some(preview) = live(s).panels.attachment() else {
                return;
            };
            let (title, description) = crate::composer::preview_header(preview);
            (
                title,
                description,
                crate::composer::preview_view(preview, s, k),
            )
        }
        Screen::Launcher => (
            "New session",
            "Choose a local checkout or run Codex or Pi on a VM.",
            launcher(s, k),
        ),
        Screen::Palette => ("Command palette", "", crate::palette::view(s, k)),
        _ => return,
    };
    let (width, height): (f32, f32) = match screen {
        Screen::Settings => (740.0, crate::settings::height(s)),
        Screen::Launcher => (560.0, 380.0),
        Screen::Palette => (580.0, 440.0),
        Screen::Worktrees => (560.0, crate::checkouts::manager_height(s)),
        Screen::RemoveVm => (
            460.0,
            if live(s).panels.removal().is_some_and(|removal| {
                live(s)
                    .app
                    .remote
                    .delete_errors
                    .contains_key(&removal.checkout)
            }) {
                320.0
            } else {
                276.0
            },
        ),
        Screen::CreateWorktree => (500.0, 330.0),
        Screen::CreateVm | Screen::RemotePicker => (556.0, crate::remote::create_height(s)),
        Screen::Timeline => (620.0, 480.0),
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
        Screen::Attachment => (
            760.0,
            live(s)
                .panels
                .attachment()
                .map_or(120.0, crate::composer::preview_height),
        ),
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
