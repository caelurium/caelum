use std::cmp::Reverse;

use caelum_core::application::FeedSession;
use rune::{El, col, row, text};
use rune_kit::Kit;

use crate::{Workspace, live::live, ui};

const MAX_RECENT: usize = 5;

pub(super) fn view(s: &Workspace, k: &Kit) -> Option<El> {
    let l = live(s);
    let project = l.app.home.draft.project.as_deref();
    let heading = match project {
        Some(path) => {
            let name = crate::checkouts::project_name(&l.app, l.app.group_path(path));
            format!("Recent in {name}")
        }
        None => "Recent in General".into(),
    };
    let items: Vec<FeedSession> = match project {
        Some(path) => l
            .app
            .sidebar_sessions(Some(path), "", false)
            .into_iter()
            .take(MAX_RECENT)
            .collect(),
        None => {
            let mut unbound: Vec<_> = l
                .app
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, session)| {
                    session.unbound && !session.archived() && !session.forgotten()
                })
                .map(|(i, session)| (Reverse((session.pinned, session.last_activity)), i))
                .collect();
            unbound.sort_unstable();
            unbound
                .into_iter()
                .take(MAX_RECENT)
                .map(|(_, i)| FeedSession::Local(i))
                .collect()
        }
    };
    if items.is_empty() {
        return None;
    }

    let mut list = col()
        .w_full()
        .gap(2.0)
        .padding(4.0)
        .radius(12.0)
        .fill(k.alpha(k.c.layer_01, 0.72))
        .border(1.0, k.c.border_base);

    for item in items {
        match item {
            FeedSession::Local(index) => {
                let session = &l.app.sessions[index];
                let title = if session.title.trim().is_empty() {
                    "Untitled session"
                } else {
                    session.title.as_str()
                };
                let pending = l.app.needs_input(session);
                let working = session.working();
                let branch = (!session.unbound)
                    .then(|| l.app.session_branch(session))
                    .flatten();
                let provider = ui::provider_name(session.scope.provider);
                let mark = if pending {
                    Some(
                        rune::el()
                            .size(6.0, 6.0)
                            .radius(3.0)
                            .fill(k.c.warning.icon)
                            .a11y_hidden(),
                    )
                } else if working {
                    Some(ui::busy_icon(&ui::CLOCK, 11.0, k))
                } else {
                    None
                };
                let status = if pending {
                    Some(("Needs input", k.c.warning.fg))
                } else if working {
                    Some(("Running", k.c.text_accent))
                } else {
                    None
                };
                list = list.child(
                    k.interactive(
                        row()
                            .w_full()
                            .h(34.0)
                            .px(10.0)
                            .gap(8.0)
                            .center_y()
                            .radius(8.0)
                            .children(mark)
                            .child(
                                text(title)
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate()
                                    .grow(1.0)
                                    .min_w(0.0),
                            )
                            .children(status.map(|(label, color)| {
                                text(label).font(k.ty.small_medium).color(color)
                            }))
                            .children(branch.map(|b| {
                                text(b)
                                    .font(k.ty.small)
                                    .color(k.c.text_muted)
                                    .max_w(140.0)
                                    .truncate()
                            }))
                            .child(text(provider).font(k.ty.small).color(k.c.text_faint)),
                    )
                    .key(format!("home-session-{index}"))
                    .role("button")
                    .label(title)
                    .focusable(true)
                    .focus_ring(1.5, 0.0, k.c.border_focus)
                    .on_press(move |s: &mut Workspace| crate::live::select_session(s, index)),
                );
            }
            FeedSession::Vm(link) => {
                let Some((checkout, _)) = link.resolve(&l.app.remote.saved) else {
                    continue;
                };
                let title = checkout.title().to_owned();
                let branch = checkout.branch().map(str::to_owned);
                let key = format!("home-vm-{}-{}", link.checkout, link.thread);
                let checkout_id = link.checkout.clone();
                let open_label = title.clone();
                list = list.child(
                    k.interactive(
                        row()
                            .w_full()
                            .h(34.0)
                            .px(10.0)
                            .gap(8.0)
                            .center_y()
                            .radius(8.0)
                            .child(
                                text(title)
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate()
                                    .grow(1.0)
                                    .min_w(0.0),
                            )
                            .children(branch.map(|b| {
                                text(b)
                                    .font(k.ty.small)
                                    .color(k.c.text_muted)
                                    .max_w(140.0)
                                    .truncate()
                            }))
                            .child(text("VM").font(k.ty.small).color(k.c.text_faint)),
                    )
                    .key(key)
                    .role("button")
                    .label(open_label)
                    .focusable(true)
                    .focus_ring(1.5, 0.0, k.c.border_focus)
                    .on_press(move |s: &mut Workspace| {
                        crate::remote::select(s, &checkout_id);
                    }),
                );
            }
        }
    }

    Some(
        col()
            .key("home-recent-sessions")
            .w_full()
            .gap(6.0)
            .child(
                text(heading)
                    .font(k.ty.small_medium)
                    .color(k.c.text_muted)
                    .px(4.0),
            )
            .child(list),
    )
}
