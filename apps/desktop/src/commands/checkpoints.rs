use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    ui::{self, Button},
};
use caelum_core::application::{Task, presentation::duration};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{ButtonVariant, Kit, ScrollBar};

pub(crate) fn open(s: &mut Workspace) {
    let Some(index) = live(s).app.selected else {
        return;
    };
    let (project, latest_tree) = {
        let session = &live(s).app.sessions[index];
        (
            session.scope.project.clone(),
            session.checkpoints.last().map(|cp| cp.tree_oid.clone()),
        )
    };
    let l = live_mut(s);
    l.commands.owner = Some(index);
    l.commands.checkpoint_selected = latest_tree.clone();
    l.panels.set_overlay(Some(Screen::Checkpoints));
    if let Some(tree_oid) = latest_tree {
        l.app.task(Task::CheckpointDiff { project, tree_oid });
    }
}

pub(crate) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some(index) = l.commands.owner.or(l.app.selected) else {
        return ui::empty(
            "No active session",
            "Select a session to inspect checkpoints.",
            k,
        );
    };
    let Some(session) = l.app.sessions.get(index) else {
        return ui::empty(
            "No active session",
            "Select a session to inspect checkpoints.",
            k,
        );
    };
    if session.checkpoints.is_empty() {
        return ui::empty(
            "No checkpoints yet",
            "Caelum automatically captures a hidden Git working-tree snapshot before each turn in local Git repositories.",
            k,
        );
    }

    let project = session.scope.project.clone();
    let selected_tree = l
        .commands
        .checkpoint_selected
        .as_deref()
        .filter(|oid| session.checkpoints.iter().any(|cp| cp.tree_oid == *oid))
        .unwrap_or_else(|| session.checkpoints.last().unwrap().tree_oid.as_str())
        .to_owned();
    let now = caelum_core::application::presentation::now();

    let mut list = col().w(240.0).h_full().min_h(0.0).gap(4.0);
    for (i, cp) in session.checkpoints.iter().rev().enumerate() {
        let active = cp.tree_oid == selected_tree;
        let short_oid: String = cp.tree_oid.chars().take(7).collect();
        let age = if cp.created_at > 0 && now >= cp.created_at {
            format!("{} ago", duration(now - cp.created_at))
        } else {
            "just now".into()
        };
        let tree_oid = cp.tree_oid.clone();
        let project_select = project.clone();
        list = list.child(
            k.interactive(
                col()
                    .w_full()
                    .padding(8.0)
                    .gap(3.0)
                    .radius(8.0)
                    .fill(if active { k.c.layer_02 } else { k.c.layer_01 })
                    .border(
                        1.0,
                        if active {
                            k.c.border_strong
                        } else {
                            k.c.border_base
                        },
                    )
                    .child(
                        text(&cp.prompt_excerpt)
                            .font(k.ty.small_medium)
                            .color(k.c.text_base)
                            .truncate(),
                    )
                    .child(
                        row()
                            .w_full()
                            .justify_between()
                            .child(ui::caption(short_oid, k))
                            .child(ui::caption(age, k)),
                    ),
            )
            .key(format!("checkpoint-item-{i}"))
            .role("button")
            .label(&cp.prompt_excerpt)
            .focusable(true)
            .on_press(move |s: &mut Workspace| {
                let l = live_mut(s);
                l.commands.checkpoint_selected = Some(tree_oid.clone());
                l.app.task(Task::CheckpointDiff {
                    project: project_select.clone(),
                    tree_oid: tree_oid.clone(),
                });
            }),
        );
    }

    let diff_task = Task::CheckpointDiff {
        project: project.clone(),
        tree_oid: selected_tree.clone(),
    };
    let restore_task = Task::RestoreCheckpoint {
        project: project.clone(),
        tree_oid: selected_tree.clone(),
    };
    let loading = l.app.services.busy(&diff_task);
    let restoring = l.app.services.busy(&restore_task);
    let checkout = l.app.services.checkouts.get(&project);
    let diff = checkout
        .and_then(|c| c.checkpoint_diff.as_ref())
        .filter(|(oid, _)| oid == &selected_tree)
        .map(|(_, diff)| diff);
    let restored = checkout
        .and_then(|c| c.checkpoint_restored.as_ref())
        .filter(|(oid, _)| oid == &selected_tree)
        .map(|(_, paths)| paths);
    let error = l
        .app
        .services
        .error(&restore_task)
        .or_else(|| l.app.services.error(&diff_task));

    let has_changes = diff.is_some_and(|d| !d.paths.is_empty());
    let project_restore = project.clone();
    let tree_restore = selected_tree.clone();
    let project_refresh = project.clone();
    let tree_refresh = selected_tree.clone();

    let mut detail = col().grow(1.0).min_w(0.0).h_full().min_h(0.0).gap(10.0);
    let header = row()
        .w_full()
        .center_y()
        .justify_between()
        .gap(8.0)
        .child(ui::caption(
            match diff {
                Some(d) if d.paths.is_empty() => "Working tree matches this checkpoint.".to_owned(),
                Some(d) => format!(
                    "{} changed {} since this checkpoint",
                    d.paths.len(),
                    if d.paths.len() == 1 { "file" } else { "files" }
                ),
                None if loading => "Computing diff…".to_owned(),
                None => "Select a checkpoint to inspect changes.".to_owned(),
            },
            k,
        ))
        .child(
            row()
                .center_y()
                .gap(6.0)
                .child(
                    Button::new("Refresh")
                        .variant(ButtonVariant::GhostMuted)
                        .disabled(loading || restoring)
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s).app.task(Task::CheckpointDiff {
                                project: project_refresh.clone(),
                                tree_oid: tree_refresh.clone(),
                            });
                        })
                        .build(k)
                        .key("checkpoint-refresh"),
                )
                .child(
                    Button::new(if restoring {
                        "Restoring…"
                    } else {
                        "Restore files"
                    })
                    .variant(ButtonVariant::Contrast)
                    .disabled(!has_changes || loading || restoring || session.working())
                    .on_press(move |s: &mut Workspace| {
                        live_mut(s).app.task(Task::RestoreCheckpoint {
                            project: project_restore.clone(),
                            tree_oid: tree_restore.clone(),
                        });
                    })
                    .build(k)
                    .key("checkpoint-restore"),
                ),
        );
    detail = detail.child(header);

    if let Some(err) = error {
        detail = detail.child(text(err).font(k.ty.small).color(k.c.danger.fg).wrap());
    }
    if let Some(paths) = restored {
        detail = detail.child(
            text(if paths.is_empty() {
                "Working tree already matched this checkpoint.".to_owned()
            } else {
                format!(
                    "Restored {} {}.",
                    paths.len(),
                    if paths.len() == 1 { "file" } else { "files" }
                )
            })
            .font(k.ty.small_medium)
            .color(k.c.success.fg)
            .wrap(),
        );
    }

    if let Some(d) = diff {
        if d.paths.is_empty() {
            detail = detail.child(ui::empty(
                "No differences",
                "Your current checkout files already match this snapshot.",
                k,
            ));
        } else {
            let files = d.paths.join("\n");
            detail = detail.child(
                col()
                    .w_full()
                    .max_h(90.0)
                    .padding(8.0)
                    .radius(8.0)
                    .fill(k.c.layer_01)
                    .border(1.0, k.c.border_base)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(text(files).font(k.ty.mono).color(k.c.text_base).wrap())
                    .scroll_bar(k),
            );
            if d.truncated {
                detail = detail.child(ui::caption("Diff preview truncated to 256 KiB.", k));
            }
            detail = detail.child(
                col()
                    .key("checkpoint-diff-body")
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .padding(10.0)
                    .radius(8.0)
                    .fill(k.c.layer_01)
                    .border(1.0, k.c.border_muted)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        text(&d.diff)
                            .font(k.ty.mono)
                            .color(k.c.text_base)
                            .wrap()
                            .selectable(true),
                    )
                    .scroll_bar(k),
            );
        }
    }

    row()
        .key("checkpoints-panel")
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(12.0)
        .child(
            col()
                .w(240.0)
                .h_full()
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list)
                .scroll_bar(k),
        )
        .child(detail)
}
