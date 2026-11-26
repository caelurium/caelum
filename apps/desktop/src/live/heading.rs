use crate::{Workspace, checkouts, panels::Screen, ui};
use rune::{El, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, glyphs};

pub(crate) fn heading(s: &Workspace, k: &Kit) -> El {
    let base = row().center_y().min_w(0.0).shrink().gap(10.0);
    if let Some(l) = &s.live
        && l.panels.screen == Screen::Conversation
        && let Some(index) = l.app.selected
    {
        return session_heading(s, k, index, false);
    }
    let project = s
        .live
        .as_ref()
        .and_then(|l| match l.panels.screen {
            Screen::Remote => l
                .app
                .remote
                .saved
                .selected
                .as_ref()
                .and_then(|id| l.app.remote.saved.checkouts.get(id))
                .and_then(|c| c.source.repository.rsplit('/').next()),
            Screen::Home | Screen::Overview => None,
            _ => l
                .app
                .active_project
                .as_ref()
                .map(|path| l.app.checkout_directory(path))
                .and_then(|path| std::path::Path::new(path).file_name())
                .and_then(|name| name.to_str()),
        })
        .unwrap_or("caelum");
    base.child(
        ui::caption(project, k)
            .max_w(160.0)
            .min_w(0.0)
            .truncate()
            .shrink(),
    )
    .child(text("/").font(k.ty.ui).color(k.c.text_faint))
    .child(
        text(s.title())
            .font(k.ty.ui_medium)
            .color(k.c.text_base)
            .truncate()
            .min_w(0.0)
            .shrink(),
    )
}

pub(crate) fn session_heading(s: &Workspace, k: &Kit, index: usize, split: bool) -> El {
    let base = row().center_y().min_w(0.0).shrink().gap(10.0);
    let Some(l) = &s.live else {
        return base;
    };
    let Some(session) = l.app.sessions.get(index) else {
        return base;
    };
    let path = &session.scope.project;
    let (project, branch, ink) = if session.unbound {
        ("General".to_owned(), None, k.c.border_strong)
    } else {
        let name = checkouts::project_name(&l.app, l.app.group_path(path));
        let project = l
            .app
            .session_worktree(session)
            .map_or_else(|| name.to_owned(), |tree| format!("{name} / {tree}"));
        let branch = l.app.session_branch(session);
        let ink = l.feed.project_ink(&l.app, path, k);
        (project, branch, ink)
    };
    let eyebrow = row()
        .center_y()
        .min_w(0.0)
        .gap(6.0)
        .child(
            text(project)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        )
        .children(branch.map(|branch| {
            row()
                .center_y()
                .gap(3.0)
                .px(5.0)
                .py(1.0)
                .radius(4.0)
                .fill(k.c.layer_02)
                .child(icon(&ui::BRANCH, 10.0).color(k.c.icon_muted).a11y_hidden())
                .child(
                    text(branch)
                        .font(rune::TextStyle::new(10.5, 450.0))
                        .color(k.c.text_muted)
                        .truncate(),
                )
        }));
    let title_body = col()
        .min_w(0.0)
        .shrink()
        .px(5.0)
        .radius(5.0)
        .child(eyebrow)
        .child(
            row()
                .center_y()
                .min_w(0.0)
                .gap(6.0)
                .child(
                    text(&session.title)
                        .font(TextStyle::new(15.0, 570.0).lh(20.0))
                        .color(k.c.text_base)
                        .min_w(0.0)
                        .shrink()
                        .truncate(),
                )
                .children(
                    (!split).then(|| icon(&glyphs::CHEVRON_DOWN, 11.0).color(k.c.icon_muted)),
                ),
        );
    let title = if split {
        title_body.key(format!("split-heading-{index}"))
    } else {
        k.interactive(title_body)
            .key("conversation-heading")
            .role("button")
            .label(format!("{} · Browse prompts", session.title))
            .help("Browse the prompts in this conversation")
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press(crate::commands::open_timeline)
    };
    let attach_btn = (!split && session.unbound).then(|| {
        let enabled = l.app.can_attach_to_project(index);
        ui::Button::new("Attach to project")
            .icon(&ui::FOLDER)
            .variant(rune_kit::ButtonVariant::GhostMuted)
            .disabled(!enabled)
            .on_press(move |s: &mut Workspace| {
                let l = super::live_mut(s);
                l.attach_menu = if l.attach_menu == Some(index) {
                    None
                } else {
                    Some(index)
                };
            })
            .build(k)
            .h(26.0)
            .px(8.0)
            .key("attach-project")
            .help("Attach this general chat to a project or worktree")
    });
    base.child(el().size(3.0, 26.0).radius(1.5).fill(ink))
        .child(title)
        .children(attach_btn)
        .children(super::approval_rules::trigger(l, index, k))
        .children(
            (!split)
                .then(|| super::attention::session_attention(s, k))
                .flatten(),
        )
}
