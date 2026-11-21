use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    quiet_button, ui,
};
use caelum_core::remote::Job;
use rune::{El, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit, glyphs};

pub(crate) fn section(s: &Workspace, k: &Kit, project: &str) -> Option<(El, f32)> {
    let l = live(s);
    let r = &l.app.remote;
    let rows: Vec<_> = r
        .saved
        .checkouts
        .iter()
        .filter(|(_, c)| l.app.group_path(&c.source.project) == project)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let parent = crate::checkouts::project_name(&l.app, project);
    let selected = r
        .saved
        .selected
        .as_ref()
        .filter(|_| l.panels.screen == Screen::Remote);
    let active = rows.iter().any(|(id, _)| Some(*id) == selected)
        || l.app
            .active_project
            .as_ref()
            .is_some_and(|p| l.app.group_path(p) == project);
    let expanded = r.saved.expanded.get(project).copied().unwrap_or(active);
    let toggle = project.to_owned();
    let open = project.to_owned();
    let mut group = ui::project_group::group(k)
        .key(crate::code_view::source_key(project, "remote-group"))
        .label(format!("Remote VMs for {parent}"))
        .child(
            row()
                .w_full()
                .h(28.0)
                .center_y()
                .gap(2.0)
                .child(
                    ui::project_group::toggle("Remote VMs", rows.len(), expanded, k)
                        .key(crate::code_view::source_key(project, "remote-toggle"))
                        .label(if expanded {
                            "Hide remote VMs"
                        } else {
                            "Show remote VMs"
                        })
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s)
                                .app
                                .remote
                                .saved
                                .expanded
                                .insert(toggle.clone(), !expanded);
                        }),
                )
                .child(
                    quiet_button(&crate::PLUS, "Open on remote machine…")
                        .on_press(move |s: &mut Workspace| super::open(s, open.clone()))
                        .build(k)
                        .size(22.0, 24.0),
                ),
        );
    let mut height = 36.0;
    if expanded {
        for (id, c) in rows {
            let status = super::status::status(r, id, c);
            let branch = super::status::branch(c);
            let title = c
                .machine()
                .map(|m| m.name.as_str())
                .or(branch)
                .unwrap_or("VM setup");
            let detail = branch.filter(|_| c.machine().is_some()).map_or_else(
                || status.label.clone(),
                |b| format!("{} · {b}", status.label),
            );
            let key = format!("remote-checkout-{id}");
            let remove_key = format!("remove-remote-{id}");
            let is_current = Some(id) == selected || super::native::selected(l) == Some(id);
            let show_action = is_current
                || l.focus_key
                    .as_ref()
                    .is_some_and(|v| v.ends_with(&remove_key));
            let remove = id.clone();
            let open = id.clone();
            let item = ui::project_group::item(title, &detail, &ui::SERVER, is_current, k)
                .key(&key)
                .label(format!("Open remote checkout {title}"))
                .description(&detail)
                .help(format!(
                    "{parent} / {title}\n{}\n{detail}",
                    c.source.repository
                ))
                .min_w(0.0)
                .grow(1.0)
                .on_press(move |s: &mut Workspace| super::select(s, &open));
            group = group.child(
                row()
                    .w_full()
                    .h(42.0)
                    .center_y()
                    .hover_group()
                    .child(crate::context::remote_checkout(item, id.clone(), key))
                    .child(
                        quiet_button(
                            &glyphs::CLOSE,
                            if c.machine().is_some() {
                                "Delete VM on Boat…"
                            } else {
                                "Remove VM setup…"
                            },
                        )
                        .disabled(!l.app.can_remove_remote(id))
                        .on_press(move |s: &mut Workspace| request_removal(s, &remove))
                        .build(k)
                        .size(22.0, 24.0)
                        .key(remove_key)
                        .label(if c.machine().is_some() {
                            format!("Delete {title} on Boat")
                        } else {
                            format!("Remove {title} setup")
                        })
                        .group_opacity(if show_action { 1.0 } else { 0.0 }, 1.0),
                    ),
            );
            height += 44.0;
        }
    }
    Some((group, height))
}

pub(crate) fn request_removal(s: &mut Workspace, id: &str) {
    let l = live_mut(s);
    if l.app.can_remove_remote(id) {
        l.remote.removing = Some((
            id.into(),
            l.app.remote.saved.checkouts[id]
                .machine()
                .map(|m| m.key.clone()),
        ));
        l.panels.overlay = Some(Screen::RemoveVm);
    }
}

pub(crate) fn removal(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some((id, c)) = l.remote.removing.as_ref().and_then(|(id, key)| {
        l.app
            .remote
            .saved
            .checkouts
            .get_key_value(id)
            .filter(|(_, c)| c.machine().map(|m| &m.key) == key.as_ref())
    }) else {
        return col();
    };
    let owner = id.clone();
    let target = c.machine().map(|m| m.key.clone());
    let deleting = matches!(l.app.remote.jobs.get(id), Some(Job::Deleting(_)));
    let confirm = if deleting {
        "Deleting…"
    } else if target.is_some() {
        "Delete VM on Boat"
    } else {
        "Remove setup"
    };
    col()
        .w_full()
        .gap(12.0)
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(icon(&ui::SERVER, 16.0).color(k.c.icon_muted))
                .child(
                    text(c.machine().map_or("VM setup", |m| &m.name))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .truncate(),
                ),
        )
        .child(ui::caption(
            format!(
                "{} · {}",
                c.source.repository,
                super::status::branch(c).unwrap_or("Remote checkout")
            ),
            k,
         ).truncate())
        .children(target.as_ref().map(|m|ui::caption(format!("Boat machine ID: {}",m.id),k).selectable(true).wrap()))
        .child(text(if target.is_some() {"Permanently delete this VM and its disk on Boat, ending any running work. Caelum will remove its VM chats and drafts. Your local session stays here."} else {"Remove this setup and its saved drafts from Caelum. No Boat machine ID was returned for this checkout."})
            .font(k.ty.ui).color(k.c.text_muted).w_full().wrap())
        .children(l.app.remote.delete_errors.get(id).map(|e| ui::caption(e,k).color(k.c.danger.fg).role("alert")))
        .child(
            row()
                .w_full()
                .justify_end()
                .gap(6.0)
                .child(
                    ui::Button::new(if deleting { "Close" } else { "Cancel" })
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(cancel_removal)
                        .build(k)
                        .key("cancel-remove-vm"),
                )
                .child(
                    ui::Button::new(confirm)
                        .variant(ButtonVariant::Danger)
                        .disabled(!l.app.can_remove_remote(id))
                        .on_press(move |s: &mut Workspace| {
                            let l=live_mut(s);
                            if l.panels.overlay==Some(Screen::RemoveVm) && l.remote.removing.as_ref().is_some_and(|(id,k)|id==&owner && k==&target) {
                                l.app.remove_remote(&owner,target.as_ref());
                                l.schedule();
                            }
                        })
                        .build(k)
                        .key("confirm-remove-vm"),
                ),
        )
}

fn cancel_removal(s: &mut Workspace) {
    let l = live_mut(s);
    l.remote.removing = None;
    l.panels.overlay = None;
    l.focus();
}

pub(crate) fn finish_removals(s: &mut Workspace) -> bool {
    let l = live_mut(s);
    let removed = std::mem::take(&mut l.app.remote.removed);
    if removed.is_empty() {
        return false;
    }
    l.remote.editors.retain(|owner, _| {
        l.app.remote.saved.checkouts.iter().any(|(id, c)| {
            std::iter::once(&c.thread)
                .chain(&c.other_threads)
                .any(|t| t.editor_key(id) == owner)
        })
    });
    if l.remote
        .removing
        .as_ref()
        .is_some_and(|(id, _)| removed.contains(id))
    {
        l.remote.removing = None;
        if l.panels.overlay == Some(Screen::RemoveVm) {
            l.panels.overlay = None;
        }
    }
    if (l.panels.screen == Screen::Remote && l.app.remote.saved.selected.is_none())
        || (l.panels.screen == Screen::Conversation
            && l.app.selected.is_none()
            && l.app.active_project.is_none())
    {
        if l.app.selected.is_some() {
            l.panels.screen = Screen::Conversation;
            l.sync_editors();
            l.focus();
            s.inspector.set_open(false);
        } else {
            crate::home::open(s);
        }
    }
    true
}
