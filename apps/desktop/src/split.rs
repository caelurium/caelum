//! Split-pane workspace layout: view two sessions side-by-side or a session beside a PR review.

use rune::{Cursor, Cx, Drag, El, Key, KeyEvent, Phase, Tx, col, row, text};
use rune_kit::{Kit, glyphs};

use crate::{
    Workspace,
    live::{live, live_mut},
    quiet_button, ui,
};

pub const COMPACT_WIDTH: f32 = 1100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivePane {
    #[default]
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SplitSide {
    Left,
    #[default]
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondaryPane {
    Session(usize),
    Review(String, String),
}

#[derive(Debug, Clone)]
pub struct SplitLayout {
    pub secondary: Option<SecondaryPane>,
    pub side: SplitSide,
    pub active: ActivePane,
    /// Fraction of the workspace allocated to the left visual pane (`0.25..=0.75`).
    pub ratio: f32,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self {
            secondary: None,
            side: SplitSide::Right,
            active: ActivePane::Primary,
            ratio: 0.5,
        }
    }
}

impl SplitLayout {
    pub fn is_open(&self) -> bool {
        self.secondary.is_some()
    }

    pub fn is_visible(&self, window_width: f32) -> bool {
        self.secondary.is_some() && window_width >= COMPACT_WIDTH
    }

    pub fn effective_active(&self, window_width: f32) -> ActivePane {
        if self.is_visible(window_width) {
            self.active
        } else {
            ActivePane::Primary
        }
    }

    pub fn open_session_on(&mut self, index: usize, side: SplitSide) {
        self.secondary = Some(SecondaryPane::Session(index));
        self.side = side;
        self.active = ActivePane::Secondary;
    }

    pub fn open_review(&mut self, project: String, url: String) {
        self.secondary = Some(SecondaryPane::Review(project, url));
        self.side = SplitSide::Right;
        self.active = ActivePane::Secondary;
    }

    pub fn close(&mut self) {
        self.secondary = None;
        self.active = ActivePane::Primary;
    }
}

pub fn composer_target(l: &crate::live::Live) -> Option<usize> {
    if l.split.effective_active(l.window_width.get()) == ActivePane::Secondary
        && let Some(SecondaryPane::Session(index)) = l.split.secondary
        && index < l.app.sessions.len()
    {
        return Some(index);
    }
    l.app.selected
}

pub fn open_session_on(s: &mut Workspace, index: usize, side: SplitSide) {
    let l = live_mut(s);
    if index >= l.app.sessions.len() {
        return;
    }
    l.split.open_session_on(index, side);
    focus_pane(s, ActivePane::Secondary);
}

pub fn open_review(s: &mut Workspace, project: String, url: String) {
    let l = live_mut(s);
    l.panels.review = Some((project.clone(), url.clone()));
    l.split.open_review(project, url);
}

pub fn swap(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(SecondaryPane::Session(secondary)) = l.split.secondary else {
        return;
    };
    let side = l.split.side;
    let primary = l.app.selected;
    l.app.select(secondary);
    if let Some(prev) = primary.filter(|&p| p != secondary) {
        l.split.open_session_on(prev, side);
        focus_pane(s, ActivePane::Primary);
    } else {
        close(s);
    }
}

pub fn close(s: &mut Workspace) {
    live_mut(s).split.close();
    focus_pane(s, ActivePane::Primary);
}

pub fn focus_pane(s: &mut Workspace, pane: ActivePane) {
    let l = live_mut(s);
    if pane == ActivePane::Secondary && !l.split.is_open() {
        return;
    }
    l.split.active = pane;
    for draft in &mut l.drafts {
        draft.focused = false;
    }
    match pane {
        ActivePane::Primary => {
            if let Some(i) = l.app.selected
                && let Some(draft) = l.drafts.get_mut(i)
            {
                draft.focused = true;
                l.focus_key = Some(format!("live-draft-{i}"));
            }
        }
        ActivePane::Secondary => {
            if let Some(SecondaryPane::Session(i)) = l.split.secondary
                && let Some(draft) = l.drafts.get_mut(i)
            {
                draft.focused = true;
                l.focus_key = Some(format!("live-draft-{i}"));
            }
        }
    }
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    let Some(l) = s.live.as_ref() else {
        return false;
    };
    if !l.split.is_visible(l.window_width.get()) {
        return false;
    }
    if ev.mods.cmd && ev.mods.alt && ev.key == Key::Right {
        let target = match l.split.side {
            SplitSide::Right => ActivePane::Secondary,
            SplitSide::Left => ActivePane::Primary,
        };
        focus_pane(s, target);
        return true;
    }
    if ev.mods.cmd && ev.mods.alt && ev.key == Key::Left {
        let target = match l.split.side {
            SplitSide::Right => ActivePane::Primary,
            SplitSide::Left => ActivePane::Secondary,
        };
        focus_pane(s, target);
        return true;
    }
    if ev.is_command('l') {
        let active = l.split.effective_active(l.window_width.get());
        focus_pane(s, active);
        return true;
    }
    if ev.key == Key::Escape
        && l.split.effective_active(l.window_width.get()) == ActivePane::Secondary
        && let Some(SecondaryPane::Session(index)) = l.split.secondary
        && l.app.sessions.get(index).is_some_and(|s| s.working())
    {
        live_mut(s).app.cancel(index);
        return true;
    }
    false
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
    let Some(secondary) = l.split.secondary.clone() else {
        return crate::live::heading(s, k);
    };
    let left_ratio = l.split.ratio.clamp(0.25, 0.75);
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

    let (sec_heading, can_swap) = match &secondary {
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
    };

    let actions = row()
        .center_y()
        .gap(2.0)
        .children(can_swap.then(|| {
            quiet_button(&ui::BACK, "Swap with primary pane")
                .on_press(swap)
                .build(k)
                .key("split-swap")
        }))
        .child(
            quiet_button(&glyphs::CLOSE, "Close split pane")
                .on_press(close)
                .build(k)
                .key("split-close"),
        );

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
        .child(actions);

    let (left_inner, right_inner) = match l.split.side {
        SplitSide::Right => (primary_slot, secondary_slot),
        SplitSide::Left => (secondary_slot, primary_slot),
    };

    let exact_left = cx
        .layout("split-left-pane")
        .zip(cx.layout("split-toolbar-heading"))
        .map(|(pane, bar)| (pane[0] + pane[2] - bar[0]).max(120.0));

    let left = match exact_left {
        Some(w) => left_inner.w(w).pr(12.0),
        None => left_inner.grow(left_ratio).pr(12.0),
    };
    let right = right_inner.grow(1.0).pl(12.0);

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

fn active_dot(k: &Kit) -> El {
    rune::el()
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(k.c.text_accent)
        .a11y_hidden()
}

pub fn composer_target_bar(s: &Workspace, k: &Kit) -> Option<El> {
    let l = s.live.as_ref()?;
    if !l.split.is_visible(l.window_width.get()) {
        return None;
    }
    let Some(SecondaryPane::Session(sec)) = l.split.secondary else {
        return None;
    };
    let prim = l.app.selected?;
    if prim == sec || prim >= l.app.sessions.len() || sec >= l.app.sessions.len() {
        return None;
    }
    let active = l.split.active;
    let ((left_pane, left_idx), (right_pane, right_idx)) = match l.split.side {
        SplitSide::Right => ((ActivePane::Primary, prim), (ActivePane::Secondary, sec)),
        SplitSide::Left => ((ActivePane::Secondary, sec), (ActivePane::Primary, prim)),
    };
    let pill = |key: &'static str, side_label: &'static str, pane: ActivePane, index: usize| {
        let session = &l.app.sessions[index];
        let selected = active == pane;
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
                .max_w(240.0)
                .min_w(0.0)
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
        .label(format!("Send to {side_label} pane: {title}"))
        .focusable(true)
        .focus_selected(selected)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press(move |s: &mut Workspace| focus_pane(s, pane))
    };

    Some(
        row()
            .key("split-composer-target")
            .w_full()
            .center_y()
            .gap(8.0)
            .pb(2.0)
            .child(
                ui::segmented_track(k)
                    .child(pill("split-target-left", "Left", left_pane, left_idx))
                    .child(pill("split-target-right", "Right", right_pane, right_idx)),
            )
            .child(rune::el().grow(1.0))
            .child(
                text(ui::shortcut("⌥⌘← / ⌥⌘→"))
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
    main_width: f32,
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
    let Some(secondary) = l.split.secondary.clone() else {
        return primary;
    };
    let left_ratio = l.split.ratio.clamp(0.25, 0.75);

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

    let secondary_el = secondary_pane(s, k, secondary, reduced, padding, pane_border, layers);

    let (left_content, right_content) = match l.split.side {
        SplitSide::Right => (primary_el, secondary_el),
        SplitSide::Left => (secondary_el, primary_el),
    };

    let left = col()
        .key("split-left-pane")
        .grow(left_ratio)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .child(left_content);

    let right = col()
        .key("split-right-pane")
        .grow(1.0 - left_ratio)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .child(right_content);

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

#[allow(clippy::too_many_arguments)]
fn secondary_pane(
    s: &Workspace,
    k: &Kit,
    secondary: SecondaryPane,
    reduced: bool,
    padding: f32,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let content = match secondary {
        SecondaryPane::Session(index) => {
            crate::live::transcript_for(s, k, index, reduced, padding, true)
        }
        SecondaryPane::Review(_, _) => crate::panels::view(s, k, layers),
    };

    col()
        .key("split-secondary-pane")
        .w_full()
        .h_full()
        .min_h(0.0)
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Secondary {
                focus_pane(s, ActivePane::Secondary);
            }
        })
        .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden())
        .child(content.grow(1.0).min_h(0.0))
}
