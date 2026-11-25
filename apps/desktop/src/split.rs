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

pub fn wrap_main(
    s: &Workspace,
    k: &Kit,
    primary: El,
    reduced: bool,
    window_width: f32,
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
    let primary_active = l.split.active == ActivePane::Primary;

    let (primary_title, primary_sub) = l
        .app
        .selected
        .and_then(|i| l.app.sessions.get(i))
        .map(|sess| (sess.title.clone(), Some(sess.scope.provider.name())))
        .unwrap_or_else(|| ("Primary".into(), None));

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
        .child(pane_header(
            &primary_title,
            primary_sub,
            primary_active,
            None,
            k,
        ))
        .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden())
        .child(primary.grow(1.0).min_h(0.0));

    let right = secondary_pane(
        s,
        k,
        secondary,
        !primary_active,
        reduced,
        pane_border,
        layers,
    )
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

fn pane_header(
    title: &str,
    subtitle: Option<&str>,
    active: bool,
    trailing: Option<El>,
    k: &Kit,
) -> El {
    row()
        .w_full()
        .h(38.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .fill(if active { k.c.bg_base } else { k.c.layer_01 })
        .child(
            rune::el()
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(if active {
                    k.c.text_accent
                } else {
                    k.alpha(k.c.text_muted, 0.35)
                })
                .a11y_hidden(),
        )
        .child(
            text(title)
                .font(k.ty.ui_medium)
                .color(if active {
                    k.c.text_base
                } else {
                    k.c.text_muted
                })
                .truncate()
                .min_w(0.0),
        )
        .children(subtitle.map(|sub| ui::caption(sub, k).truncate()))
        .child(rune::el().grow(1.0))
        .children(trailing)
}

fn secondary_pane(
    s: &Workspace,
    k: &Kit,
    secondary: SecondaryPane,
    active: bool,
    reduced: bool,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let (title, subtitle, can_swap) = match &secondary {
        SecondaryPane::Session(index) => live(s)
            .app
            .sessions
            .get(*index)
            .map(|sess| (sess.title.clone(), Some(sess.scope.provider.name()), true))
            .unwrap_or_else(|| ("Session".into(), None, false)),
        SecondaryPane::Review(_, url) => (
            format!(
                "PR Review · {}",
                url.rsplit('/').next().unwrap_or(url.as_str())
            ),
            None,
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

    let content = match secondary {
        SecondaryPane::Session(index) => secondary_session(s, k, index, reduced, layers),
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
        .child(pane_header(&title, subtitle, active, Some(actions), k))
        .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden())
        .child(content.grow(1.0).min_h(0.0))
}

fn secondary_session(
    s: &Workspace,
    k: &Kit,
    index: usize,
    reduced: bool,
    layers: &mut Vec<El>,
) -> El {
    let padding = 20.0;
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .items_center()
        .child(crate::live::transcript_for(
            s, k, index, reduced, padding, true,
        ))
        .child(crate::live::composer_for(
            s, k, index, padding, false, true, layers,
        ))
}
