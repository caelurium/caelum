use super::{live, live_mut};
use crate::{PLUS, Workspace, quiet_button, ui};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{Kit, ScrollBar, glyphs, shell::SidebarHeader};

pub(super) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let app = &l.app;
    let roots: Vec<_> = app
        .projects
        .iter()
        .filter(|p| app.group_path(&p.path) == p.path)
        .collect();
    let mut projects = col()
        .key("projects-list")
        .role("list")
        .w_full()
        .gap(4.0)
        .focus_group();
    let mut height = (roots.len() as f32 * 36.0 - 4.0).max(32.0);
    for project in &roots {
        let path = project.path.clone();
        let selected = app
            .active_project
            .as_ref()
            .is_some_and(|p| app.group_path(p) == path);
        let create_key = crate::code_view::source_key(&path, "create-worktree");
        let close_key = crate::code_view::source_key(&path, "close-project");
        let mut group = col()
            .w_full()
            .key(crate::code_view::source_key(&path, "project-group"))
            .role("group")
            .label(format!("{} project and checkouts", project.name))
            .child(crate::context::project(
                k.interactive(
                    row()
                        .w_full()
                        .h(32.0)
                        .px(8.0)
                        .gap(7.0)
                        .center_y()
                        .radius(6.0)
                        .fill(if selected {
                            k.c.hover
                        } else {
                            rune::Color::TRANSPARENT
                        })
                        .child(l.feed.badges.view(app, &path, k))
                        .child(
                            text(&project.name)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate()
                                .grow(1.0)
                                .min_w(0.0),
                        )
                        .child(
                            quiet_button(&ui::BRANCH, "Create worktree")
                                .on_press({
                                    let path = path.clone();
                                    move |s: &mut Workspace| {
                                        crate::worktree::open(
                                            s,
                                            path.clone(),
                                            live(s).app.preferences.default_provider,
                                        );
                                    }
                                })
                                .build(k)
                                .size(22.0, 24.0)
                                .key(create_key),
                        )
                        .child(
                            quiet_button(&glyphs::CLOSE, "Close project")
                                .on_press({
                                    let path = path.clone();
                                    move |s: &mut Workspace| live_mut(s).close_project(&path)
                                })
                                .build(k)
                                .size(22.0, 24.0)
                                .key(close_key)
                                .label(format!("Close {}", project.name)),
                        ),
                )
                .role("button")
                .label(&project.name)
                .description(&path)
                .help(&path)
                .focus_group()
                .focusable(true)
                .focus_selected(selected)
                .focus_return(if roots.len() == 1 {
                    "open-project"
                } else {
                    "projects-list"
                })
                .focus_ring(1.0, 0.0, k.c.border_focus)
                .key(crate::code_view::source_key(&path, "project"))
                .on_press({
                    let path = path.clone();
                    move |s: &mut Workspace| {
                        crate::checkouts::select_project(s, &path);
                    }
                }),
                path.clone(),
            ));
        if let Some((trees, tree_height)) = crate::checkouts::sidebar(s, k, &path) {
            group = group.child(trees);
            height += tree_height;
        }
        if let Some((vms, vm_height)) = crate::remote::sidebar(s, k, &path) {
            group = group.child(vms);
            height += vm_height;
        }
        projects = projects.child(group);
    }
    col()
        .key("projects-section")
        .w_full()
        .grow(0.65)
        .min_h((height + 32.0).min(100.0))
        .max_h((height + 32.0).min(280.0))
        .gap(4.0)
        .child(
            SidebarHeader::new(if app.opening_project() {
                "Opening project…"
            } else {
                "Projects"
            })
            .count(roots.len())
            .action(
                row()
                    .gap(1.0)
                    .child(crate::panels::navigation(s, k))
                    .child(
                        quiet_button(&PLUS, "Open project")
                            .on_press_tx(|s: &mut Workspace, tx| {
                                super::super::open_project(s, tx);
                            })
                            .build(k)
                            .key("open-project"),
                    )
                    .children(ui::sidebar_collapse(s, k)),
            )
            .build(k)
            .key("projects-heading"),
        )
        .child(
            col()
                .key("projects-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(if roots.is_empty() {
                    ui::caption("Open a project to get started", k).padding(8.0)
                } else {
                    projects
                })
                .scroll_bar(k),
        )
}
