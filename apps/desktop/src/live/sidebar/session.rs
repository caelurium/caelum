use super::live;
use crate::{Workspace, quiet_button, ui};
use caelum_core::application::presentation::ActivityKind;
use caelum_protocol::GitHubPullRequest;
use rune::{El, icon, row, text};
use rune_kit::{Kit, glyphs};

pub(super) fn view(s: &Workspace, k: &Kit, index: usize, position: usize, count: usize) -> El {
    let l = live(s);
    let app = &l.app;
    let session = &app.sessions[index];
    let selected = app.selected == Some(index)
        && !matches!(
            l.panels.screen,
            crate::panels::Screen::Home | crate::panels::Screen::Overview
        );
    let error = session.error.as_deref().or_else(|| {
        session
            .conversation
            .failure
            .notice()
            .map(|n| n.message.as_str())
    });
    let project = crate::checkouts::project_name(app, app.group_path(&session.scope.project));
    let branch = app.session_branch(session).unwrap_or("No branch");
    let branch = app
        .session_worktree(session)
        .map_or_else(|| branch.to_owned(), |tree| format!("{tree} · {branch}"));
    let pending = app.needs_input(session);
    let activity = crate::activity::session_activity(s, index);
    let waiting = !session.archiving()
        && !session.deleting()
        && activity
            .as_ref()
            .is_some_and(|activity| activity.kind == ActivityKind::Background);
    let subagents =
        waiting && caelum_core::application::subagents::running(&session.conversation.entries) > 0;
    let status = if pending {
        "Needs input"
    } else if session.archiving() {
        "Archiving…"
    } else if session.deleting() {
        "Deleting…"
    } else if session.stopping() {
        "Stopping…"
    } else if waiting {
        activity.as_ref().unwrap().label.as_str()
    } else if session.working() {
        "Working…"
    } else if error.is_some() {
        "Error"
    } else if session.loading() {
        "Loading…"
    } else if session.unread {
        "Unread"
    } else {
        ""
    };
    let provider = ui::provider_name(session.scope.provider);
    let age = super::age(session.last_activity, l.feed.displayed_at()).0;
    let ink = if pending {
        k.c.warning.icon
    } else if waiting {
        k.c.text_accent
    } else if error.is_some() {
        k.c.danger.fg
    } else {
        k.c.text_muted
    };
    let mark = if session.working() && !pending && !waiting {
        ui::busy_icon(&ui::CLOCK, 10.0, k)
    } else {
        rune::el()
            .size(5.0, 5.0)
            .radius(3.0)
            .fill(if pending || waiting {
                ink
            } else if error.is_some() {
                k.c.danger.fg
            } else {
                k.c.text_accent
            })
    };
    let action_visible = selected
        || l.focus_key
            .as_ref()
            .is_some_and(|key| key.ends_with(&format!("session-actions-{index}")));
    let detail = if pending || waiting {
        text(if pending || subagents {
            status
        } else {
            "Waiting on commands"
        })
        .key(if pending {
            format!("session-input-{index}")
        } else {
            format!("session-waiting-{index}")
        })
        .role("status")
        .label(status)
        .help(status)
        .color(ink)
    } else {
        text(&branch).color(k.c.text_muted)
    };
    let mut bottom = row()
        .w_full()
        .gap(4.0)
        .center_y()
        .child(
            icon(
                if pending {
                    &ui::BELL
                } else if waiting {
                    &ui::CLOCK
                } else {
                    &ui::BRANCH
                },
                11.0,
            )
            .color(if pending || waiting {
                ink
            } else {
                k.c.icon_muted
            }),
        )
        .child(detail.font(k.ty.small).truncate().min_w(0.0).grow(1.0))
        .children(app.session_pull_request(session).map(|pr| {
            pull_request_button(pr, &session.scope.project, k)
                .key(format!("session-pull-request-{index}"))
        }))
        .child(
            quiet_button(&ui::MORE, "Session actions")
                .on_press(move |s: &mut Workspace| super::live_mut(s).session_menu = Some(index))
                .build(k)
                .size(20.0, 20.0)
                .key(format!("session-actions-{index}"))
                .group_opacity(if action_visible { 1.0 } else { 0.0 }, 1.0),
        );
    if !super::super::archive_disabled(l, index) {
        bottom = bottom.child(
            quiet_button(&glyphs::CLOSE, "Close")
                .on_press(move |s: &mut Workspace| {
                    let l = super::live_mut(s);
                    if l.app.sessions[index].archived() {
                        l.request_delete_session(index);
                    } else {
                        l.archive(index, true);
                    }
                })
                .build(k)
                .size(20.0, 20.0)
                .key("close")
                .focusable(false)
                .group_opacity(0.0, 1.0),
        );
    }
    let mark = if session.pinned && status.is_empty() {
        Some(icon(&ui::PIN, 10.0).color(k.c.text_accent).a11y_hidden())
    } else if !status.is_empty() {
        Some(
            row()
                .center_y()
                .gap(4.0)
                .children(
                    session
                        .pinned
                        .then(|| icon(&ui::PIN, 10.0).color(k.c.text_accent).a11y_hidden()),
                )
                .child(mark.a11y_hidden()),
        )
    } else {
        None
    };
    let item = super::row::Card {
        badge: l.feed.badges.view(app, &session.scope.project, k),
        project,
        title: &session.title,
        provider,
        age: &age,
        ink,
        mark,
    }
    .build(k, bottom, selected, position, count)
    .description(format!("{project}, {branch}, {provider}, {status}, {age}"))
    .help(format!(
        "{}\n{branch}\n{provider} · {status}\n{}",
        app.checkout_directory(&session.scope.project),
        error.unwrap_or("")
    ))
    .key(format!("live-session-{index}"))
    .on_press(move |s: &mut Workspace| super::select(s, index));
    crate::context::session(ui::selected_row(item, selected, k).mb(4.0), index)
}

fn pull_request_button(pr: &GitHubPullRequest, project: &str, k: &Kit) -> El {
    let (kind, ink) = if pr.is_draft {
        ("draft pull request", k.c.text_muted)
    } else {
        ("pull request", k.c.success.fg)
    };
    let number = format!("#{}", pr.number);
    let label = format!("Open {kind} {number}: {}", pr.title);
    let project = project.to_owned();
    let url = pr.url.clone();
    k.interactive(
        row()
            .h(20.0)
            .px(4.0)
            .gap(4.0)
            .center_y()
            .radius(4.0)
            .child(icon(&ui::PULL_REQUEST, 16.0).color(ink))
            .child(text(number).font(k.ty.small_medium).color(ink)),
    )
    .role("button")
    .label(&label)
    .help(label)
    .focusable(true)
    .focus_ring(1.0, 0.0, k.c.border_focus)
    .on_press(move |s: &mut Workspace| crate::panels::open_review(s, project.clone(), url.clone()))
}
