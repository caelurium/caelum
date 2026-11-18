use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    ui::{self, Button},
};
use caelum_core::application::{ForkBoundary, ForkPoint};
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
        ForkBoundary::OpenCodeBefore(id) => format!("fork-before-{id}"),
        ForkBoundary::OpenCodeTip(id) => format!("fork-tip-{id}"),
    }
}

pub(super) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some(picker) = l.app.fork_picker() else {
        return col();
    };
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
        .label("Fork points")
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
                ui::choice(label, &detail, i == active, &kit)
                    .on_press(move |s: &mut Workspace| live_mut(s).app.fork_at(&boundary))
            },
        );
    let mut body = col().w_full().h_full().min_h(0.0).gap(8.0).child(
        ui::field(&l.commands.search, "Find a fork point", "Find a turn…", k)
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
        "Creating fork…"
    } else if picker.loading() {
        "Loading history…"
    } else {
        "↑↓ Choose a turn · ↵ Fork"
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
                        l.app.open_fork(i);
                    }
                })
                .build(k),
        );
    }
    body.child(ui::divider(k)).child(footer)
}

pub fn fork_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
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
