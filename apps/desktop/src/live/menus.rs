use rune::{El, col, icon, row, text};
use rune_kit::{ButtonVariant, Dialog, Kit, Placement, glyphs};

use crate::ui::Button;
use crate::{Workspace, quiet_button, ui};

use super::{Live, live, live_mut};

pub fn new_general_session(s: &mut Workspace) {
    let l = live_mut(s);
    l.schedule();
    l.reveal_new_session();
    let provider = l.app.preferences.default_provider;
    let index = l.app.new_general_session(provider);
    l.sync_editors();
    l.drafts[index].focused = true;
    l.menu = None;
    l.attach_menu = None;
    l.panels.screen = crate::panels::Screen::Conversation;
}

fn session_action_row(
    title: &str,
    detail: &str,
    glyph: &'static rune::Icon,
    danger: bool,
    disabled: bool,
    k: &Kit,
) -> El {
    let ink = if danger { k.c.danger.fg } else { k.c.text_base };
    let icon_ink = if danger {
        k.c.danger.fg
    } else {
        k.c.icon_muted
    };
    k.interactive(
        row()
            .w_full()
            .min_h(40.0)
            .px(10.0)
            .py(6.0)
            .gap(10.0)
            .center_y()
            .radius(6.0)
            .child(icon(glyph, 14.0).color(icon_ink).a11y_hidden())
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(title).font(k.ty.ui_medium).color(ink).truncate())
                    .child(ui::caption(detail, k).truncate()),
            ),
    )
    .role("menuitem")
    .label(title)
    .description(detail)
    .disabled(disabled)
    .focusable(!disabled)
    .focus_highlight(k.c.hover)
    .focus_ring(1.5, 0.0, k.c.border_focus)
    .opacity(if disabled { 0.5 } else { 1.0 })
}

fn session_actions(s: &Workspace, k: &Kit) -> El {
    let live = live(s);
    let Some(index) = live.session_menu else {
        return col();
    };
    let session = &live.app.sessions[index];
    let refresh_disabled = session.remote_id.is_none() || session.loading() || session.submitting();
    let mut content = col()
        .w_full()
        .gap(4.0)
        .child(ui::popup_header(&session.title, None, None, k))
        .children((!session.archived()).then(|| {
            session_action_row(
                if session.pinned {
                    "Unpin session"
                } else {
                    "Pin session"
                },
                if session.pinned {
                    "Return this session to normal ordering"
                } else {
                    "Keep this session at the top of Sessions"
                },
                &ui::PIN,
                false,
                false,
                k,
            )
            .on_press(move |s: &mut Workspace| {
                live_mut(s).app.toggle_pin(index);
                close_session_menu(s);
            })
            .key("pin-session")
        }))
        .child(
            session_action_row(
                "Refresh session",
                if session.loading() {
                    "Reconnecting and loading history…"
                } else if session.remote_id.is_none() {
                    "Available after your first message"
                } else {
                    "Reconnect and load the latest history"
                },
                &ui::REFRESH,
                false,
                refresh_disabled,
                k,
            )
            .on_press(move |s: &mut Workspace| {
                live_mut(s).app.refresh_session(index);
                close_session_menu(s);
            })
            .key("refresh-history"),
        );
    if session.unbound {
        let can_attach = live.app.can_attach_to_project(index);
        content = content.child(
            session_action_row(
                "Attach to project…",
                if can_attach {
                    "Bind this general chat to a project or worktree"
                } else {
                    "Finish active work before attaching to a project"
                },
                &ui::FOLDER,
                false,
                !can_attach,
                k,
            )
            .on_press(move |s: &mut Workspace| {
                close_session_menu(s);
                let l = live_mut(s);
                l.app.select(index);
                l.panels.screen = crate::panels::Screen::Conversation;
                l.attach_menu = Some(index);
            })
            .key("attach-session-project"),
        );
    }
    let archive_disabled = archive_disabled(live, index);
    content = content
        .child(
            session_action_row(
                "Split vertical",
                "Open this session side-by-side",
                &rune_kit::shell::INSPECTOR,
                false,
                false,
                k,
            )
            .on_press(move |s: &mut Workspace| {
                crate::split::open_session(s, index, crate::split::SplitOrientation::Vertical);
                close_session_menu(s);
            })
            .key("split-session-vertical"),
        )
        .child(
            session_action_row(
                "Split horizontal",
                "Open this session stacked below",
                &rune_kit::shell::INSPECTOR,
                false,
                false,
                k,
            )
            .on_press(move |s: &mut Workspace| {
                crate::split::open_session(s, index, crate::split::SplitOrientation::Horizontal);
                close_session_menu(s);
            })
            .key("split-session-horizontal"),
        );
    content = content.child(
        session_action_row(
            if session.archived() {
                "Restore session"
            } else {
                "Archive session"
            },
            if archive_disabled {
                "Finish active work, requests and queued messages first"
            } else {
                "Keep its history and return to it later"
            },
            &ui::ARCHIVE,
            false,
            archive_disabled,
            k,
        )
        .on_press(move |s: &mut Workspace| {
            let archived = crate::live::live(s).app.sessions[index].archived();
            live_mut(s).archive(index, !archived);
        })
        .key("archive-session"),
    );
    if session.archived() {
        content = content.child(ui::divider(k)).child(
            session_action_row(
                if session.deleting() {
                    "Deleting session…"
                } else {
                    "Delete permanently…"
                },
                "Delete this session and its provider history",
                &glyphs::CLOSE,
                true,
                archive_disabled || session.deleting(),
                k,
            )
            .on_press(move |s: &mut Workspace| {
                live_mut(s).request_delete_session(index);
            })
            .key("delete-session"),
        );
    }
    content
}

pub(crate) fn archive_disabled(live: &Live, index: usize) -> bool {
    let session = &live.app.sessions[index];
    session.working()
        || session.archiving()
        || session.deleting()
        || !session.queue.is_empty()
        || !session.inbox.items.is_empty()
        || live.app.approvals.iter().any(|p| {
            p.project_path == session.scope.project
                && p.provider == session.scope.provider.as_str()
                && p.session_id == session.remote_id
        })
        || live.app.questions.iter().any(|p| {
            p.project_path == session.scope.project
                && p.provider == session.scope.provider.as_str()
                && Some(&p.session_id) == session.remote_id.as_ref()
        })
}

pub fn close_session_menu(s: &mut Workspace) {
    let l = live_mut(s);
    if let Some(index) = l.session_menu.take() {
        l.focus_key = Some(format!("session-actions-{index}"));
    }
}

pub fn overlays(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    super::approval_rules::overlay(s, k, layers);
    super::sidebar::overlays(s, k, layers);
    let l = live(s);
    if let Some(index) = l.app.last_archived() {
        let session = &l.app.sessions[index];
        let width =
            rune::memo::window_layout("main-pane").map_or(480.0, |r| (r[2] - 32.0).min(480.0));
        let toast = row()
            .key("archive-toast")
            .role("status")
            .label("Session archived")
            .w(width)
            .padding(10.0)
            .gap(10.0)
            .center_y()
            .on_hover_change(|s: &mut Workspace, inside, _| live_mut(s).archive_hovered = inside)
            .child(icon(&ui::ARCHIVE, 15.0).color(k.c.icon_muted))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(3.0)
                    .child(
                        text("Session archived")
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base),
                    )
                    .child(ui::caption(&session.title, k).truncate())
                    .children(
                        session
                            .error
                            .as_ref()
                            .map(|e| ui::caption(e, k).color(k.c.danger.fg)),
                    ),
            )
            .child(
                Button::new(if session.archiving() {
                    "Restoring…"
                } else {
                    "Undo"
                })
                .variant(ButtonVariant::Outline)
                .disabled(session.archiving())
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    if l.app.last_archived() == Some(index) {
                        l.app.undo_archive();
                        l.sync_editors();
                    }
                })
                .build(k)
                .h(28.0)
                .px(10.0)
                .key("undo-archive"),
            )
            .child(
                quiet_button(&glyphs::CLOSE, "Dismiss archive notice")
                    .on_press(move |s: &mut Workspace| live_mut(s).app.dismiss_archive(index))
                    .build(k)
                    .key("dismiss-archive"),
            );
        let x =
            rune::memo::window_layout("main-pane").map_or(16.0, |r| r[0] + (r[2] - width) / 2.0);
        let bottom = rune::memo::window_layout("message-composer")
            .zip(rune::memo::window_layout("shell"))
            .map_or(18.0, |(composer, shell)| shell[3] - composer[1] + 12.0);
        layers.push(rune_kit::popover::raised(k, toast, ui::POPUP_RADIUS).pin(
            Some(x),
            None,
            None,
            Some(bottom),
        ));
    }
    if let Some(index) = l.session_menu {
        ui::popup(
            "session-menu",
            &format!("session-actions-{index}"),
            session_actions(s, k).w(300.0),
            Placement::BottomStart,
            close_session_menu,
            k,
            layers,
        );
    }
    attach_project_overlay(s, k, layers);
}

fn attach_project_overlay(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(index) = live(s).attach_menu else {
        return;
    };
    let l = live(s);
    if !l.app.sessions.get(index).is_some_and(|s| s.unbound) {
        return;
    }
    let mut list = col().w_full().gap(2.0);
    for project in &l.app.projects {
        let path = project.path.clone();
        let detail = l
            .app
            .services
            .worktrees(l.app.group_path(&path))
            .and_then(|listing| listing.worktrees.iter().find(|t| t.project.path == path))
            .and_then(|t| t.branch.as_deref())
            .map_or_else(|| path.clone(), |branch| format!("{branch} · {path}"));
        list = list.child(
            ui::choice(&project.name, &detail, false, k)
                .key(format!("attach-project-{}", project.path))
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    l.attach_menu = None;
                    if l.app.attach_to_project(index, path.clone()) {
                        l.schedule();
                        l.focus();
                    }
                }),
        );
    }
    let content = col()
        .w(320.0)
        .gap(6.0)
        .child(ui::popup_header(
            "Attach to project",
            Some("Keep this conversation and continue in a project"),
            None,
            k,
        ))
        .children((!l.app.projects.is_empty()).then(|| {
            list.max_h(240.0).scroll(rune::ScrollSpec {
                stick_bottom: false,
            })
        }))
        .child(
            Button::new("Open project folder…")
                .icon(&ui::FOLDER)
                .variant(ButtonVariant::GhostMuted)
                .on_press_tx(move |s: &mut Workspace, tx: &mut rune::Tx| {
                    live_mut(s).attach_menu = None;
                    tx.open_folder(
                        rune::FileDialogOptions {
                            title: "Choose a project to attach".into(),
                            ..Default::default()
                        },
                        move |s: &mut Workspace, result, _| {
                            if let Ok(Some(path)) = result {
                                let path = path.to_string_lossy().into_owned();
                                let l = live_mut(s);
                                l.app.open_project(path.clone());
                                if l.app.attach_to_project(index, path) {
                                    l.schedule();
                                    l.focus();
                                }
                            }
                        },
                    );
                })
                .build(k)
                .key("attach-open-folder"),
        );
    ui::popup(
        "attach-project-menu",
        "attach-project",
        content,
        Placement::BottomStart,
        |s: &mut Workspace| live_mut(s).attach_menu = None,
        k,
        layers,
    );
}

pub fn delete_confirmation(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(index) = live(s).delete_confirmation else {
        return;
    };
    let Some(session) = live(s).app.sessions.get(index) else {
        return;
    };
    let title = session.title.clone();
    let description = if session.remote_id.is_some() {
        format!(
            "Delete “{title}”, its provider history and any child sessions? This cannot be undone."
        )
    } else {
        format!(
            "Permanently remove “{title}” and its unsent draft and attachments? This cannot be undone."
        )
    };
    Dialog::new("delete-session", "Delete session permanently?")
        .open(true)
        .click_outside(false)
        .width(460.0)
        .fit()
        .on_close(|s: &mut Workspace| live_mut(s).cancel_delete_session())
        .child(
            col()
                .w_full()
                .gap(16.0)
                .px(16.0)
                .pb(16.0)
                .child(
                    text(description)
                        .key("delete-session-description")
                        .font(k.ty.ui.lh(20.0))
                        .color(k.c.text_muted)
                        .wrap()
                        .w_full(),
                )
                .child(
                    row()
                        .w_full()
                        .justify_end()
                        .gap(8.0)
                        .child(
                            crate::ui::Button::new("Cancel")
                                .variant(ButtonVariant::GhostMuted)
                                .on_press(|s: &mut Workspace| live_mut(s).cancel_delete_session())
                                .build(k)
                                .key("cancel-delete-session"),
                        )
                        .child(
                            crate::ui::Button::new("Delete permanently")
                                .variant(ButtonVariant::Contrast)
                                .on_press(move |s: &mut Workspace| {
                                    live_mut(s).confirm_delete_session(index)
                                })
                                .build(k)
                                .key("confirm-delete-session"),
                        ),
                ),
        )
        .build(k, layers);
}
