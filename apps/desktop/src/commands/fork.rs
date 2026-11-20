use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    ui::{self, Button},
};
use caelum_core::application::{FilePreview, ForkBoundary, ForkPoint, HistoryAction, Provider};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row};
use rune_kit::{ButtonVariant, Kit};

fn matches(s: &Workspace) -> Vec<&ForkPoint> {
    let l = live(s);
    let query = l.commands.search.text.to_lowercase();
    l.app
        .fork_picker()
        .into_iter()
        .flat_map(|p| &p.points)
        .filter(|p| p.label.to_lowercase().contains(&query))
        .collect()
}

fn row_key(boundary: &ForkBoundary) -> String {
    match boundary {
        ForkBoundary::CodexTurn(id) => format!("fork-turn-{id}"),
        ForkBoundary::CodexBefore(id) => format!("before-turn-{id}"),
        ForkBoundary::OpenCodeBefore(id) => format!("fork-before-{id}"),
        ForkBoundary::OpenCodeTip(id) => format!("fork-tip-{id}"),
    }
}

pub(crate) fn header(s: &Workspace) -> (&'static str, &'static str) {
    let l = live(s);
    let provider = l.app.selected.map(|i| l.app.sessions[i].scope.provider);
    match (l.app.fork_picker().map(|p| p.action), provider) {
        (Some(HistoryAction::Edit), Some(Provider::Codex)) => (
            "Edit in a branch",
            "Codex 0.145+: branch before the prompt, edit its draft, then send. The original stays intact.",
        ),
        (Some(HistoryAction::Edit), _) => (
            "Edit in a branch",
            "Branch before the prompt, edit its draft, then send. The original stays intact.",
        ),
        (Some(HistoryAction::Rewind), Some(Provider::Codex)) => (
            "Rewind conversation",
            "Codex 0.148+ paginated threads: remove history from a chosen turn. Checkout files stay unchanged.",
        ),
        (Some(HistoryAction::Rewind), _) => (
            "Rewind conversation",
            "Choose the first prompt to remove. File restoration is optional and uses OpenCode snapshots.",
        ),
        _ => (
            "Fork conversation",
            "Keep the history through the chosen turn in a new session.",
        ),
    }
}

pub(super) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some(picker) = l.app.fork_picker() else {
        return col();
    };
    let owner = picker.token();
    if let Some(point) = picker.confirmation() {
        let n = point.newer_turns + 1;
        let mut body = col()
            .w_full()
            .gap(12.0)
            .child(
                ui::caption(
                    format!(
                        "Remove this prompt and the history after it ({n} user turn{}):",
                        if n == 1 { "" } else { "s" }
                    ),
                    k,
                )
                .wrap(),
            )
            .child(ui::caption(&point.label, k).wrap())
            .child(
                Button::new("Rewind conversation only")
                    .on_press(move |s: &mut Workspace| confirm(s, owner, false))
                    .build(k)
                    .key("rewind-history"),
            );
        match picker.file_preview() {
            Some(FilePreview::Available(paths)) if !paths.is_empty() => {
                let description = format!(
                    "OpenCode reports {} changed paths. Restoring its snapshot can also affect later edits in this checkout.",
                    paths.len()
                );
                let names = paths
                    .iter()
                    .take(12)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join("\n");
                body = body
                    .child(ui::caption(description, k).wrap())
                    .child(
                        Button::new("Rewind and restore files")
                            .variant(ButtonVariant::Outline)
                            .on_press(move |s: &mut Workspace| confirm(s, owner, true))
                            .build(k)
                            .key("rewind-files"),
                    )
                    .child(ui::caption(names, k).wrap());
                if paths.len() > 12 {
                    body = body.child(ui::caption(format!("…and {} more", paths.len() - 12), k));
                }
            }
            Some(FilePreview::Available(_)) => {
                body = body.child(ui::caption(
                    "No file changes reported for this checkpoint.",
                    k,
                ))
            }
            Some(FilePreview::Loading(_)) => {
                body = body.child(ui::caption("Reading snapshot changes…", k))
            }
            Some(FilePreview::Unavailable(e)) => {
                body =
                    body.child(ui::caption(format!("File restoration unavailable: {e}"), k).wrap())
            }
            _ => {}
        }
        return col()
            .w_full()
            .h_full()
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(
                body.children(
                    picker
                        .error
                        .as_ref()
                        .map(|e| ui::caption(e, k).color(k.c.danger.fg).wrap()),
                ),
            );
    }
    let points = matches(s);
    let active = points
        .iter()
        .position(|p| Some(&p.boundary) == l.commands.fork_selected.as_ref())
        .unwrap_or(0);
    let items: Vec<_> = points
        .iter()
        .map(|p| (p.label.clone(), p.boundary.clone(), p.newer_turns))
        .collect();
    let keys: Vec<_> = items.iter().map(|(_, b, _)| row_key(b)).collect();
    let kit = k.clone();
    let rows = col()
        .key("fork-rows")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .role("list")
        .label("Conversation turns")
        .virtual_rows(
            items.len(),
            48.0,
            move |_: &Workspace, i| keys[i].clone().into(),
            move |_: &Workspace, i| {
                let (label, boundary, newer) = &items[i];
                let boundary = boundary.clone();
                let detail = match newer {
                    0 => "Latest turn".into(),
                    1 => "1 turn earlier".into(),
                    n => format!("{n} turns earlier"),
                };
                ui::choice(label, &detail, i == active, &kit).on_press(move |s: &mut Workspace| {
                    if live(s)
                        .app
                        .fork_picker()
                        .is_some_and(|p| p.token() == owner)
                    {
                        live_mut(s).app.fork_at(&boundary)
                    }
                })
            },
        );
    let mut body = col().w_full().h_full().min_h(0.0).gap(8.0).child(
        ui::field(&l.commands.search, "Find a turn", "Find a turn…", k)
            .w_full()
            .single_line()
            .edit_menu_changed(
                "fork-search",
                |s| Some(&mut live_mut(s).commands.search),
                |s, tx| {
                    live_mut(s).commands.fork_selected = None;
                    tx.scroll_to_y("fork-points", 0.0);
                },
            )
            .on_input_key(fork_key),
    );
    if points.is_empty() && !picker.loading() && picker.error.is_none() {
        body = body.child(ui::empty(
            "No matching turns",
            "Try a different search or load an earlier page.",
            k,
        ));
    }
    body = body.child(
        col()
            .w_full()
            .min_h(0.0)
            .grow(1.0)
            .key("fork-points")
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows),
    );
    if let Some(error) = &picker.error {
        body = body.child(ui::caption(error, k).color(k.c.danger.fg).wrap());
    }
    let status = if picker.creating() {
        if picker.action == HistoryAction::Rewind {
            "Rewinding…"
        } else {
            "Creating branch…"
        }
    } else if picker.loading() {
        "Loading history…"
    } else {
        match picker.action {
            HistoryAction::Fork => "↑↓ Choose a turn · ↵ Fork",
            HistoryAction::Edit => "↑↓ Choose a prompt · ↵ Create editable branch",
            HistoryAction::Rewind => "↑↓ Choose a prompt · ↵ Preview rewind",
        }
    };
    let mut footer = row()
        .w_full()
        .center_y()
        .justify_between()
        .child(ui::caption(status, k));
    if picker.has_older() {
        footer = footer.child(
            Button::new("Earlier turns")
                .variant(ButtonVariant::Outline)
                .disabled(picker.loading() || picker.creating())
                .on_press(|s: &mut Workspace| {
                    let l = live_mut(s);
                    l.commands.fork_selected = None;
                    l.commands.search.clear();
                    l.app.older_fork_points();
                })
                .build(k),
        );
    } else if picker.error.is_some() {
        footer = footer.child(
            Button::new("Reload turns")
                .variant(ButtonVariant::Outline)
                .on_press(|s: &mut Workspace| {
                    let l = live_mut(s);
                    if let Some(i) = l.app.selected {
                        l.commands.fork_selected = None;
                        l.app.reload_history_action(i);
                    }
                })
                .build(k),
        );
    }
    body.child(ui::divider(k)).child(footer)
}

pub fn fork_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if live(s)
        .app
        .fork_picker()
        .is_some_and(|p| p.confirmation().is_some())
    {
        return false;
    }
    let points = matches(s);
    if points.is_empty() {
        return matches!(ev.key, Key::Enter | Key::Up | Key::Down);
    }
    let current = points
        .iter()
        .position(|p| Some(&p.boundary) == live(s).commands.fork_selected.as_ref())
        .unwrap_or(0);
    let new = match ev.key {
        Key::Enter => {
            let boundary = points[current].boundary.clone();
            live_mut(s).app.fork_at(&boundary);
            return true;
        }
        Key::Up => current.saturating_sub(1),
        Key::Down => (current + 1).min(points.len() - 1),
        _ => return false,
    };
    let chosen = points[new].boundary.clone();
    tx.scroll_center_in_instant("fork-rows", row_key(&chosen));
    live_mut(s).commands.fork_selected = Some(chosen);
    true
}

fn confirm(s: &mut Workspace, owner: u64, files: bool) {
    if live(s)
        .app
        .fork_picker()
        .is_some_and(|p| p.token() == owner)
    {
        live_mut(s).app.confirm_rewind(files);
    }
}
