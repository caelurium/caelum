use crate::{
    Workspace,
    live::{Live, live, live_mut},
    ui,
};
use caelum_core::remote::{Checkout, Job, Phase};
use rune::{El, ScrollSpec, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit, ScrollBar};

/// Only the dock changes while Boat creates the machine and prepares its branch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dock {
    Setup,
    Draft,
}

impl Dock {
    pub(super) fn for_checkout(l: &Live, id: &str, c: &Checkout) -> Self {
        if l.remote
            .open_when_ready
            .as_ref()
            .is_some_and(|(owner, _)| owner == id)
        {
            return Self::Setup;
        }
        match &c.phase {
            Phase::Ready { .. } => Self::Draft,
            Phase::Prepare {
                machine,
                new_branch: None,
            } if matches!(machine.state.as_str(), "archived" | "stopped") => Self::Draft,
            _ => Self::Setup,
        }
    }
}

pub(super) fn view(s: &Workspace, k: &Kit, id: &str, c: &Checkout, padding: f32) -> El {
    let app = &live(s).app;
    let job = app.remote.jobs.get(id);
    let detail = match job {
        Some(Job::Failed(e)) => e.as_str(),
        _ => match &c.phase {
            Phase::Create(_) => "The editor will open when your VM and its branch are ready.",
            Phase::Prepare { machine, .. } if machine.state == "cloning" => {
                "Boat is fetching the repository from GitHub."
            }
            Phase::Prepare { machine, .. }
                if matches!(machine.state.as_str(), "stopped" | "archived") =>
            {
                "Resume the machine to finish opening this checkout."
            }
            _ => "Caelum is checking the repository and branch before opening the editor.",
        },
    };
    let max_height =
        rune::memo::window_layout("main-pane").map_or(144.0, |r| (r[3] * 0.24).clamp(52.0, 144.0));
    let repository = super::status::branch(c).map_or_else(
        || c.source.repository.clone(),
        |branch| format!("{} · {branch}", c.source.repository),
    );
    let card = col()
        .key("remote-setup-composer")
        .role("group")
        .label("Remote checkout setup")
        .focus_return(format!("remote-draft-{}", c.thread.editor_key(id)))
        .gap(8.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(10.0)
                .child(
                    row()
                        .size(32.0, 32.0)
                        .center()
                        .radius(7.0)
                        .fill(k.c.layer_02)
                        .child(icon(&ui::SERVER, 16.0).color(k.c.icon_accent)),
                )
                .child(
                    col()
                        .min_w(0.0)
                        .grow(1.0)
                        .gap(3.0)
                        .child(
                            text(c.machine().map_or("New remote checkout", |m| &m.name))
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(ui::caption(&repository, k).truncate().help(repository)),
                ),
        )
        .child(ui::divider(k))
        .child(
            super::status::status(&app.remote, id, c)
                .view(k)
                .key("remote-setup-status"),
        )
        .child(
            col()
                .w_full()
                .min_h(0.0)
                .max_h(max_height)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(ui::caption(detail, k).selectable(matches!(job, Some(Job::Failed(_)))))
                .scroll_bar(k),
        )
        .children(
            (!matches!(job, Some(Job::Busy(_) | Job::Wait(_) | Job::Deleting(_))))
                .then(|| actions(s, k, id, c)),
        );
    ui::composer_dock(card, padding, k)
}

fn actions(s: &Workspace, k: &Kit, id: &str, c: &Checkout) -> El {
    let app = &live(s).app;
    let retry = id.to_owned();
    let mut actions = row().w_full().center_y().flex_wrap().gap(6.0).child(
        ui::Button::new("Refresh checkout")
            .icon(&ui::REFRESH)
            .on_press(move |s: &mut Workspace| live_mut(s).app.refresh_remote(&retry))
            .build(k)
            .key("refresh-remote-checkout"),
    );
    if let Some(m) = c.machine() {
        let resume = matches!(m.state.as_str(), "archived" | "stopped");
        let owner = id.to_owned();
        actions = actions.child(
            ui::Button::new(if resume {
                "Resume machine"
            } else {
                "Stop machine"
            })
            .variant(if resume {
                ButtonVariant::Contrast
            } else {
                ButtonVariant::GhostMuted
            })
            .on_press(move |s: &mut Workspace| live_mut(s).app.power_remote(&owner, resume))
            .build(k),
        );
    }
    if matches!(app.remote.jobs.get(id), Some(Job::Failed(_))) {
        let owner = id.to_owned();
        actions = actions.child(
            ui::Button::new(if c.machine().is_some() {
                "Delete VM on Boat…"
            } else {
                "Remove from project…"
            })
            .variant(ButtonVariant::GhostMuted)
            .disabled(!app.can_remove_remote(id))
            .on_press(move |s: &mut Workspace| super::request_removal(s, &owner))
            .build(k)
            .key("remove-failed-checkout"),
        );
    }
    actions
}
