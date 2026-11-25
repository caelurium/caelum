use crate::{Workspace, checkouts, panels::Screen, ui};
use rune::{El, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, glyphs};

pub(crate) fn heading(s: &Workspace, k: &Kit) -> El {
    let base = row().center_y().min_w(0.0).shrink().gap(10.0);
    if let Some(l) = &s.live
        && l.panels.screen == Screen::Conversation
        && let Some(index) = l.app.selected
    {
        let session = &l.app.sessions[index];
        let path = &session.scope.project;
        let name = checkouts::project_name(&l.app, l.app.group_path(path));
        let project = l
            .app
            .session_worktree(session)
            .map_or_else(|| name.to_owned(), |tree| format!("{name} / {tree}"));
        let branch = l.app.session_branch(session);
        let ink = l.feed.project_ink(&l.app, path, k);
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
        let title = k
            .interactive(
                col()
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
                            .child(icon(&glyphs::CHEVRON_DOWN, 11.0).color(k.c.icon_muted)),
                    ),
            )
            .key("conversation-heading")
            .role("button")
            .label(format!("{} · Browse prompts", session.title))
            .help("Browse the prompts in this conversation")
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press(crate::commands::open_timeline);
        return base
            .child(el().size(3.0, 26.0).radius(1.5).fill(ink))
            .child(title)
            .children(super::approval_rules::trigger(l, index, k))
            .children(super::attention::session_attention(s, k));
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
