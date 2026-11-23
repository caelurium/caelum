use crate::{
    Workspace, checkouts,
    live::{Live, live},
    ui,
};
use rune::{Color, El, col, icon, row, text};
use rune_kit::Kit;

pub(super) fn count(l: &Live) -> usize {
    l.app
        .projects
        .iter()
        .filter(|p| l.app.group_path(&p.path) == p.path)
        .filter_map(|p| l.app.services.worktrees(&p.path))
        .map(|list| list.worktrees.len())
        .sum()
}

pub(super) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let app = &l.app;
    let query = l.overview.search.text.trim().to_lowercase();
    let roots: Vec<_> = app
        .projects
        .iter()
        .filter(|p| app.group_path(&p.path) == p.path)
        .collect();
    if roots.is_empty() {
        return ui::empty(
            "No open projects",
            "Open a Git repository to inspect and create worktrees here.",
            k,
        );
    }
    let mut rows = col()
        .key("overview-worktrees")
        .role("list")
        .label("Worktrees")
        .w_full()
        .gap(12.0);
    let mut total_matched = 0usize;
    for root in roots {
        let Some(listing) = app.services.worktrees(&root.path) else {
            continue;
        };
        let matched: Vec<_> = listing
            .worktrees
            .iter()
            .filter(|tree| {
                if query.is_empty() {
                    return true;
                }
                let name = checkouts::name(tree);
                let branch = tree.branch.as_deref().unwrap_or("");
                format!("{} {name} {branch} {}", root.name, tree.path)
                    .to_lowercase()
                    .contains(&query)
            })
            .collect();
        if matched.is_empty() {
            continue;
        }
        total_matched += matched.len();
        let create_path = root.path.clone();
        let manage_path = root.path.clone();
        let mut section =
            col().w_full().gap(4.0).child(
                row()
                    .w_full()
                    .min_h(38.0)
                    .px(10.0)
                    .py(5.0)
                    .gap(8.0)
                    .center_y()
                    .radius(8.0)
                    .fill(k.c.layer_01)
                    .border(1.0, k.alpha(k.c.border_base, 0.7))
                    .child(l.feed.project_badge(app, &root.path, k))
                    .child(
                        text(&root.name)
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base)
                            .grow(1.0)
                            .min_w(0.0)
                            .truncate(),
                    )
                    .child(
                        k.interactive(
                            row().h(26.0).px(9.0).center_y().radius(6.0).child(
                                text("Manage").font(k.ty.small_medium).color(k.c.text_muted),
                            ),
                        )
                        .role("button")
                        .label(format!("Manage worktrees for {}", root.name))
                        .focusable(true)
                        .focus_ring(1.5, 0.0, k.c.border_focus)
                        .on_press(move |s: &mut Workspace| {
                            checkouts::manage(s, manage_path.clone());
                        }),
                    )
                    .child(
                        k.interactive(
                            row()
                                .h(26.0)
                                .px(9.0)
                                .gap(5.0)
                                .center_y()
                                .radius(6.0)
                                .fill(k.c.layer_02)
                                .border(1.0, k.c.border_strong)
                                .child(icon(&crate::PLUS, 11.0).color(k.c.text_base).a11y_hidden())
                                .child(
                                    text("New worktree")
                                        .font(k.ty.small_medium)
                                        .color(k.c.text_base),
                                ),
                        )
                        .role("button")
                        .label(format!("New worktree in {}", root.name))
                        .focusable(true)
                        .focus_ring(1.5, 0.0, k.c.border_focus)
                        .on_press(move |s: &mut Workspace| {
                            crate::worktree::open(
                                s,
                                create_path.clone(),
                                live(s).app.preferences.default_provider,
                            );
                        }),
                    ),
            );
        for tree in matched {
            let title = checkouts::name(tree);
            let branch = tree.branch.as_deref().unwrap_or("Detached HEAD");
            let status = checkouts::status(app, tree);
            let path = tree.project.path.clone();
            let active = app.active_project.as_deref() == Some(path.as_str());
            section = section.child(
                k.interactive(
                    row()
                        .w_full()
                        .px(12.0)
                        .py(9.0)
                        .gap(10.0)
                        .center_y()
                        .radius(8.0)
                        .fill(if active {
                            k.c.hover
                        } else {
                            Color::TRANSPARENT
                        })
                        .child(icon(&ui::BRANCH, 13.0).color(k.c.icon_muted).a11y_hidden())
                        .child(
                            row()
                                .grow(1.0)
                                .min_w(0.0)
                                .center_y()
                                .gap(8.0)
                                .child(
                                    text(title)
                                        .font(k.ty.ui_medium)
                                        .color(k.c.text_base)
                                        .truncate()
                                        .shrink()
                                        .min_w(0.0),
                                )
                                .child(
                                    text(branch)
                                        .font(k.ty.small)
                                        .color(k.c.text_muted)
                                        .truncate()
                                        .shrink()
                                        .min_w(0.0),
                                ),
                        )
                        .child(
                            text(status)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .px(8.0)
                                .py(2.0)
                                .radius(10.0)
                                .fill(k.c.layer_01)
                                .border(1.0, k.c.border_base),
                        ),
                )
                .key(crate::code_view::source_key(
                    &tree.path,
                    "overview-worktree",
                ))
                .role("button")
                .label(format!("{title} ({branch})"))
                .help(&tree.path)
                .focusable(true)
                .focus_ring(1.5, 0.0, k.c.border_focus)
                .on_press(move |s: &mut Workspace| {
                    checkouts::select(s, &path);
                }),
            );
        }
        rows = rows.child(section);
    }
    if total_matched == 0 {
        return ui::empty(
            if query.is_empty() {
                "No worktrees loaded"
            } else {
                "No matching worktrees"
            },
            if query.is_empty() {
                "Worktrees across your open projects appear here once discovered."
            } else {
                "Try a different worktree label, branch, or project name."
            },
            k,
        );
    }
    rows
}
