use rune::{Cursor, Cx, Drag, El, Phase, Tx, col, row, text};
use rune_kit::{Kit, glyphs};

use crate::{
    Workspace,
    live::{live, live_mut},
    quiet_button, ui,
};

use super::{
    ActivePane, SecondaryPane, SplitOrientation, bring_to_view, close, focus_pane, rotate_forward,
    swap, toggle_orientation,
};

fn active_dot(k: &Kit) -> El {
    rune::el()
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(k.c.text_accent)
        .a11y_hidden()
}

fn pane_actions(s: &Workspace, k: &Kit, can_swap: bool) -> El {
    let l = live(s);
    let offscreen = l.split.offscreen_count();
    let toggle_label = match l.split.orientation {
        SplitOrientation::Vertical => "Switch to horizontal split",
        SplitOrientation::Horizontal => "Switch to vertical split",
    };
    row()
        .center_y()
        .gap(2.0)
        .children((offscreen > 0).then(|| {
            quiet_button(
                &ui::REFRESH,
                &format!("Rotate split deck ({offscreen} off-screen · ⌥⌘])"),
            )
            .on_press(rotate_forward)
            .build(k)
            .key("split-rotate")
        }))
        .children(can_swap.then(|| {
            quiet_button(&ui::BACK, "Swap split panes")
                .on_press(swap)
                .build(k)
                .key("split-swap")
        }))
        .child(
            quiet_button(&rune_kit::shell::INSPECTOR, toggle_label)
                .on_press(toggle_orientation)
                .build(k)
                .key("split-toggle-orientation"),
        )
        .child(
            quiet_button(&glyphs::CLOSE, "Close split pane")
                .on_press(close)
                .build(k)
                .key("split-close"),
        )
}

fn secondary_title(s: &Workspace, k: &Kit, secondary: &SecondaryPane) -> (El, bool) {
    match secondary {
        SecondaryPane::Session(index) => (crate::live::session_heading(s, k, *index, true), true),
        SecondaryPane::Review(_, url) => (
            text(format!(
                "PR Review · {}",
                url.rsplit('/').next().unwrap_or(url.as_str())
            ))
            .font(k.ty.ui_medium)
            .color(k.c.text_base)
            .truncate()
            .min_w(0.0),
            false,
        ),
        SecondaryPane::Overview => (
            text("Overview")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate()
                .min_w(0.0),
            false,
        ),
    }
}

pub fn toolbar_heading(
    s: &Workspace,
    k: &Kit,
    cx: &Cx<Workspace>,
    window_width: f32,
    pane_border: rune::Color,
) -> El {
    let Some(l) = s.live.as_ref() else {
        return crate::live::heading(s, k);
    };
    if !l.split.is_visible(window_width) {
        return crate::live::heading(s, k);
    }
    let Some(secondary) = l.split.visible_secondary().cloned() else {
        return crate::live::heading(s, k);
    };
    let primary_active = l.split.active == ActivePane::Primary;

    let primary_slot = row()
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .opacity(if primary_active { 1.0 } else { 0.62 })
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Primary {
                focus_pane(s, ActivePane::Primary);
            }
        })
        .children(primary_active.then(|| active_dot(k)))
        .child(crate::live::heading(s, k));

    if l.split.orientation == SplitOrientation::Horizontal {
        return primary_slot.grow(1.0);
    }

    let left_ratio = l.split.ratio.clamp(0.25, 0.75);
    let (sec_heading, can_swap) = secondary_title(s, k, &secondary);
    let secondary_slot = row()
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .opacity(if !primary_active { 1.0 } else { 0.62 })
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Secondary {
                focus_pane(s, ActivePane::Secondary);
            }
        })
        .children((!primary_active).then(|| active_dot(k)))
        .child(sec_heading)
        .child(rune::el().grow(1.0))
        .child(pane_actions(s, k, can_swap));

    let exact_left = cx
        .layout("split-left-pane")
        .zip(cx.layout("split-toolbar-heading"))
        .map(|(pane, bar)| (pane[0] + pane[2] - bar[0]).max(120.0));

    let left = match exact_left {
        Some(w) => primary_slot.w(w).pr(12.0),
        None => primary_slot.grow(left_ratio).pr(12.0),
    };
    let right = secondary_slot.grow(1.0).pl(12.0);

    row()
        .key("split-toolbar-heading")
        .w_full()
        .grow(1.0)
        .min_w(0.0)
        .center_y()
        .child(left)
        .child(rune::el().w(1.0).h(26.0).fill(pane_border).a11y_hidden())
        .child(right)
}

pub fn composer_target_bar(s: &Workspace, k: &Kit) -> Option<El> {
    let l = s.live.as_ref()?;
    if !l.split.is_visible(l.window_width.get()) {
        return None;
    }
    let Some(SecondaryPane::Session(sec)) = l.split.visible_secondary().cloned() else {
        return None;
    };
    let prim = l.app.selected?;
    if prim == sec || prim >= l.app.sessions.len() || sec >= l.app.sessions.len() {
        return None;
    }
    let active = l.split.active;
    let pill = |key: String,
                aria_prefix: &'static str,
                index: usize,
                selected: bool,
                offscreen: bool,
                on_click: Box<dyn Fn(&mut Workspace)>| {
        let session = &l.app.sessions[index];
        let title = if session.title.trim().is_empty() {
            "Untitled session"
        } else {
            session.title.as_str()
        };
        k.interactive(
            row()
                .h(24.0)
                .px(8.0)
                .gap(6.0)
                .center_y()
                .radius(6.0)
                .max_w(200.0)
                .min_w(0.0)
                .opacity(if offscreen { 0.65 } else { 1.0 })
                .fill(if selected {
                    k.c.layer_02
                } else {
                    rune::Color::TRANSPARENT
                })
                .border(
                    1.0,
                    if selected {
                        k.c.border_base
                    } else {
                        rune::Color::TRANSPARENT
                    },
                )
                .children(selected.then(|| active_dot(k)))
                .child(
                    text(title)
                        .font(if selected {
                            k.ty.small_medium
                        } else {
                            k.ty.small
                        })
                        .color(if selected {
                            k.c.text_base
                        } else {
                            k.c.text_muted
                        })
                        .truncate()
                        .min_w(0.0),
                )
                .children(
                    session
                        .working()
                        .then(|| ui::busy_icon(&ui::CLOCK, 11.0, k)),
                ),
        )
        .key(key)
        .role("tab")
        .label(format!("{aria_prefix}: {title}"))
        .focusable(true)
        .focus_selected(selected)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press(move |s: &mut Workspace| on_click(s))
    };

    let mut track = ui::segmented_track(k)
        .child(pill(
            "split-target-primary".into(),
            "Send to primary pane",
            prim,
            active == ActivePane::Primary,
            false,
            Box::new(|s| focus_pane(s, ActivePane::Primary)),
        ))
        .child(pill(
            "split-target-secondary".into(),
            "Send to secondary pane",
            sec,
            active == ActivePane::Secondary,
            false,
            Box::new(|s| focus_pane(s, ActivePane::Secondary)),
        ));

    let offscreen: Vec<usize> = l
        .split
        .deck
        .iter()
        .skip(1)
        .filter_map(|p| match p {
            SecondaryPane::Session(i) if *i < l.app.sessions.len() => Some(*i),
            _ => None,
        })
        .collect();
    if !offscreen.is_empty() {
        track = track.child(
            rune::el()
                .w(1.0)
                .h(14.0)
                .fill(k.c.border_base)
                .a11y_hidden(),
        );
        for idx in offscreen {
            track = track.child(pill(
                format!("split-target-offscreen-{idx}"),
                "Rotate off-screen pane into view",
                idx,
                false,
                true,
                Box::new(move |s| bring_to_view(s, idx)),
            ));
        }
    }

    let hint = match (l.split.orientation, l.split.offscreen_count() > 0) {
        (SplitOrientation::Vertical, true) => "⌥⌘←/→ · ⌥⌘[/] rotate",
        (SplitOrientation::Vertical, false) => "⌥⌘← / ⌥⌘→",
        (SplitOrientation::Horizontal, true) => "⌥⌘↑/↓ · ⌥⌘[/] rotate",
        (SplitOrientation::Horizontal, false) => "⌥⌘↑ / ⌥⌘↓",
    };

    Some(
        row()
            .key("split-composer-target")
            .w_full()
            .center_y()
            .gap(8.0)
            .pb(2.0)
            .child(track)
            .child(rune::el().grow(1.0))
            .child(
                text(ui::shortcut(hint))
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            ),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn wrap_main(
    s: &Workspace,
    k: &Kit,
    primary: El,
    reduced: bool,
    window_width: f32,
    main_size: (f32, f32),
    padding: f32,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let Some(l) = s.live.as_ref() else {
        return primary;
    };
    if !l.split.is_visible(window_width) {
        return primary;
    }
    let Some(secondary) = l.split.visible_secondary().cloned() else {
        return primary;
    };
    let ratio = l.split.ratio.clamp(0.25, 0.75);
    let orientation = l.split.orientation;

    let primary_el = col()
        .key("split-primary-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Primary {
                focus_pane(s, ActivePane::Primary);
            }
        })
        .child(primary.grow(1.0).min_h(0.0));

    let secondary_el = secondary_pane(
        s,
        k,
        secondary,
        orientation,
        reduced,
        padding,
        pane_border,
        layers,
    );

    match orientation {
        SplitOrientation::Vertical => {
            let main_width = main_size.0;
            let left = col()
                .key("split-left-pane")
                .grow(ratio)
                .min_w(0.0)
                .h_full()
                .min_h(0.0)
                .clip_rounded(0.0)
                .child(primary_el);
            let right = col()
                .key("split-right-pane")
                .grow(1.0 - ratio)
                .min_w(0.0)
                .h_full()
                .min_h(0.0)
                .clip_rounded(0.0)
                .child(secondary_el);
            let divider = row()
                .key("split-divider")
                .w(5.0)
                .h_full()
                .justify_center()
                .cursor(Cursor::ResizeColumn)
                .role("separator")
                .label("Resize split panes")
                .help("Drag to resize · Double-click to reset")
                .child(rune::el().w(1.0).h_full().fill(pane_border).a11y_hidden())
                .on_drag(move |s: &mut Workspace, d: &mut Drag, tx: &mut Tx| {
                    let l = live_mut(s);
                    match d.phase {
                        Phase::Down => d.data = l.split.ratio,
                        Phase::Move => {
                            tx.instant();
                            let total = main_width.max(400.0);
                            l.split.ratio = (d.data + d.dx / total).clamp(0.25, 0.75);
                        }
                        Phase::Up => {
                            if d.cancelled() {
                                l.split.ratio = d.data;
                            }
                        }
                    }
                })
                .on_double_press(|s: &mut Workspace| {
                    live_mut(s).split.ratio = 0.5;
                });
            row()
                .key("split-workspace")
                .w_full()
                .h_full()
                .min_h(0.0)
                .child(left)
                .child(divider)
                .child(right)
        }
        SplitOrientation::Horizontal => {
            let main_height = main_size.1;
            let top = col()
                .key("split-top-pane")
                .grow(ratio)
                .w_full()
                .min_h(0.0)
                .clip_rounded(0.0)
                .child(primary_el);
            let bottom = col()
                .key("split-bottom-pane")
                .grow(1.0 - ratio)
                .w_full()
                .min_h(0.0)
                .clip_rounded(0.0)
                .child(secondary_el);
            let divider = col()
                .key("split-divider")
                .w_full()
                .h(5.0)
                .justify_center()
                .cursor(Cursor::ResizeRow)
                .role("separator")
                .label("Resize split panes")
                .help("Drag to resize · Double-click to reset")
                .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden())
                .on_drag(move |s: &mut Workspace, d: &mut Drag, tx: &mut Tx| {
                    let l = live_mut(s);
                    match d.phase {
                        Phase::Down => d.data = l.split.ratio,
                        Phase::Move => {
                            tx.instant();
                            let total = main_height.max(300.0);
                            l.split.ratio = (d.data + d.dy / total).clamp(0.25, 0.75);
                        }
                        Phase::Up => {
                            if d.cancelled() {
                                l.split.ratio = d.data;
                            }
                        }
                    }
                })
                .on_double_press(|s: &mut Workspace| {
                    live_mut(s).split.ratio = 0.5;
                });
            col()
                .key("split-workspace")
                .w_full()
                .h_full()
                .min_h(0.0)
                .child(top)
                .child(divider)
                .child(bottom)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn secondary_pane(
    s: &Workspace,
    k: &Kit,
    secondary: SecondaryPane,
    orientation: SplitOrientation,
    reduced: bool,
    padding: f32,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let (sec_heading, can_swap) = secondary_title(s, k, &secondary);
    let content = match secondary {
        SecondaryPane::Session(index) => {
            crate::live::transcript_for(s, k, index, reduced, padding, true)
        }
        SecondaryPane::Review(_, _) => crate::panels::view(s, k, layers),
        SecondaryPane::Overview => crate::overview::view(s, k, layers),
    };
    let secondary_active = live(s).split.active == ActivePane::Secondary;

    let mut pane = col()
        .key("split-secondary-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Secondary {
                focus_pane(s, ActivePane::Secondary);
            }
        });
    match orientation {
        SplitOrientation::Vertical => {
            pane = pane.child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden());
        }
        SplitOrientation::Horizontal => {
            pane = pane
                .child(
                    row()
                        .key("split-bottom-header")
                        .w_full()
                        .h(34.0)
                        .px(16.0)
                        .gap(6.0)
                        .center_y()
                        .opacity(if secondary_active { 1.0 } else { 0.7 })
                        .children(secondary_active.then(|| active_dot(k)))
                        .child(sec_heading)
                        .child(rune::el().grow(1.0))
                        .child(pane_actions(s, k, can_swap)),
                )
                .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden());
        }
    }
    pane.child(content.grow(1.0).min_h(0.0))
}
