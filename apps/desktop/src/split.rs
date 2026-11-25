//! Split-pane workspace layout: view two sessions side-by-side or a session beside a PR review.

use rune::{El, Key, KeyEvent, Tx, col, row, text};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondaryPane {
    Session(usize),
    Review(String, String),
}

#[derive(Debug, Clone)]
pub struct SplitLayout {
    pub secondary: Option<SecondaryPane>,
    pub active: ActivePane,
    pub ratio: f32,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self {
            secondary: None,
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

    pub fn open_session(&mut self, index: usize) {
        self.secondary = Some(SecondaryPane::Session(index));
        self.active = ActivePane::Secondary;
    }

    pub fn open_review(&mut self, project: String, url: String) {
        self.secondary = Some(SecondaryPane::Review(project, url));
        self.active = ActivePane::Secondary;
    }

    pub fn close(&mut self) {
        self.secondary = None;
        self.active = ActivePane::Primary;
    }
}

pub fn open_session(s: &mut Workspace, index: usize) {
    let l = live_mut(s);
    if index >= l.app.sessions.len() {
        return;
    }
    l.split.open_session(index);
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
    let primary = l.app.selected;
    l.app.select(secondary);
    if let Some(prev) = primary.filter(|&p| p != secondary) {
        l.split.open_session(prev);
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
                l.focus_key = Some(format!("split-draft-{i}"));
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
        focus_pane(s, ActivePane::Secondary);
        return true;
    }
    if ev.mods.cmd && ev.mods.alt && ev.key == Key::Left {
        focus_pane(s, ActivePane::Primary);
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

pub fn toolbar_heading(s: &Workspace, k: &Kit, window_width: f32, pane_border: rune::Color) -> El {
    let Some(l) = s.live.as_ref() else {
        return crate::live::heading(s, k);
    };
    if !l.split.is_visible(window_width) {
        return crate::live::heading(s, k);
    }
    let Some(secondary) = l.split.secondary.clone() else {
        return crate::live::heading(s, k);
    };
    let ratio = l.split.ratio.clamp(0.3, 0.7);
    let primary_active = l.split.active == ActivePane::Primary;

    let left = row()
        .grow(ratio)
        .min_w(0.0)
        .pr(12.0)
        .center_y()
        .opacity(if primary_active { 1.0 } else { 0.72 })
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Primary {
                focus_pane(s, ActivePane::Primary);
            }
        })
        .child(crate::live::heading(s, k));

    let (right_heading, can_swap) = match &secondary {
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

    let right = row()
        .grow(1.0 - ratio)
        .min_w(0.0)
        .pl(12.0)
        .gap(8.0)
        .center_y()
        .opacity(if !primary_active { 1.0 } else { 0.72 })
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Secondary {
                focus_pane(s, ActivePane::Secondary);
            }
        })
        .child(right_heading)
        .child(rune::el().grow(1.0))
        .child(actions);

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

#[allow(clippy::too_many_arguments)]
pub fn wrap_main(
    s: &Workspace,
    k: &Kit,
    primary: El,
    reduced: bool,
    window_width: f32,
    padding: f32,
    wide: bool,
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
    let ratio = l.split.ratio.clamp(0.3, 0.7);

    let left = col()
        .key("split-primary-pane")
        .grow(ratio)
        .min_w(0.0)
        .h_full()
        .min_h(0.0)
        .on_press(|s: &mut Workspace| {
            if live(s).split.active != ActivePane::Primary {
                focus_pane(s, ActivePane::Primary);
            }
        })
        .child(primary.grow(1.0).min_h(0.0));

    let right = secondary_pane(s, k, secondary, reduced, padding, wide, pane_border, layers)
        .grow(1.0 - ratio)
        .min_w(0.0)
        .h_full()
        .min_h(0.0);

    row()
        .key("split-workspace")
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(left)
        .child(rune::el().w(1.0).h_full().fill(pane_border).a11y_hidden())
        .child(right)
}

#[allow(clippy::too_many_arguments)]
fn secondary_pane(
    s: &Workspace,
    k: &Kit,
    secondary: SecondaryPane,
    reduced: bool,
    padding: f32,
    wide: bool,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let content = match secondary {
        SecondaryPane::Session(index) => {
            secondary_session(s, k, index, reduced, padding, wide, layers)
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

fn secondary_session(
    s: &Workspace,
    k: &Kit,
    index: usize,
    reduced: bool,
    padding: f32,
    wide: bool,
    layers: &mut Vec<El>,
) -> El {
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .items_center()
        .child(crate::live::transcript_for(
            s, k, index, reduced, padding, true,
        ))
        .child(crate::live::composer_for(
            s, k, index, padding, wide, true, layers,
        ))
}
