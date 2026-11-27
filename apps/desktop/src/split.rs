//! Split-pane workspace layout: vertical/horizontal splits with an off-screen rotatable deck.

mod view;

pub use view::{composer_target_bar, toolbar_heading, wrap_main};

use rune::{Key, KeyEvent, Tx};

use crate::{
    Workspace,
    live::{live, live_mut},
};

pub const COMPACT_WIDTH: f32 = 1100.0;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActivePane {
    #[default]
    Primary,
    Secondary,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SplitOrientation {
    /// Side-by-side columns (`left | right`).
    #[default]
    Vertical,
    /// Stacked rows (`top / bottom`).
    Horizontal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecondaryPane {
    Session(usize),
    Review(String, String),
    Overview,
}

#[derive(Debug, Clone)]
pub struct SplitLayout {
    /// Ordered deck of secondary panes. `deck[0]` is currently visible beside the primary pane;
    /// `deck[1..]` sit off-screen and can be rotated back into view.
    pub deck: Vec<SecondaryPane>,
    pub orientation: SplitOrientation,
    pub active: ActivePane,
    /// Fraction of the workspace allocated to the primary pane (`0.25..=0.75`).
    pub ratio: f32,
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self {
            deck: Vec::new(),
            orientation: SplitOrientation::Vertical,
            active: ActivePane::Primary,
            ratio: 0.5,
        }
    }
}

impl SplitLayout {
    pub fn is_open(&self) -> bool {
        !self.deck.is_empty()
    }

    pub fn visible_secondary(&self) -> Option<&SecondaryPane> {
        self.deck.first()
    }

    pub fn offscreen_count(&self) -> usize {
        self.deck.len().saturating_sub(1)
    }

    pub fn is_visible(&self, window_width: f32) -> bool {
        if self.deck.is_empty() {
            return false;
        }
        match self.orientation {
            SplitOrientation::Vertical => window_width >= COMPACT_WIDTH,
            SplitOrientation::Horizontal => window_width >= 480.0,
        }
    }

    pub fn effective_active(&self, window_width: f32) -> ActivePane {
        if self.is_visible(window_width) {
            self.active
        } else {
            ActivePane::Primary
        }
    }

    pub fn push_pane(&mut self, pane: SecondaryPane, orientation: SplitOrientation) {
        self.deck.retain(|p| p != &pane);
        self.deck.insert(0, pane);
        self.orientation = orientation;
        self.active = ActivePane::Secondary;
    }

    pub fn close_visible(&mut self) {
        if !self.deck.is_empty() {
            self.deck.remove(0);
        }
        if self.deck.is_empty() {
            self.active = ActivePane::Primary;
        }
    }
}

pub fn composer_target(l: &crate::live::Live) -> Option<usize> {
    if matches!(
        l.panels.screen,
        crate::panels::Screen::Home | crate::panels::Screen::Remote
    ) {
        return None;
    }
    let primary = (l.panels.screen == crate::panels::Screen::Conversation)
        .then_some(l.app.selected)
        .flatten()
        .filter(|&i| i < l.app.sessions.len());
    let secondary = l
        .split
        .is_visible(l.window_width.get())
        .then(|| match l.split.visible_secondary() {
            Some(SecondaryPane::Session(i)) if *i < l.app.sessions.len() => Some(*i),
            _ => None,
        })
        .flatten();
    match l.split.effective_active(l.window_width.get()) {
        ActivePane::Secondary => secondary.or(primary),
        ActivePane::Primary => primary.or(secondary),
    }
}

/// Opens `index` in a split with the given `orientation`.
/// If two sessions are already visible, the previous primary slides off-screen into the deck
/// so it can be rotated back in later.
pub fn open_session(s: &mut Workspace, index: usize, orientation: SplitOrientation) {
    let l = live_mut(s);
    if index >= l.app.sessions.len() {
        return;
    }
    if l.app.selected == Some(index) {
        l.split.orientation = orientation;
        focus_pane(s, ActivePane::Primary);
        return;
    }
    if let Some(SecondaryPane::Session(current_sec)) = l.split.deck.first().cloned()
        && current_sec != index
    {
        let prev_primary = l.app.selected;
        l.split.deck.remove(0);
        l.app.select(current_sec);
        if let Some(prev) = prev_primary.filter(|&p| p != current_sec && p != index) {
            l.split.deck.retain(|p| p != &SecondaryPane::Session(prev));
            l.split.deck.push(SecondaryPane::Session(prev));
        }
    }
    l.split
        .push_pane(SecondaryPane::Session(index), orientation);
    focus_pane(s, ActivePane::Secondary);
}

pub fn open_project(s: &mut Workspace, project: &str, orientation: SplitOrientation) {
    let index = {
        let l = live_mut(s);
        let Some(index) = l.app.project_session_for_split(project) else {
            return;
        };
        l.sync_editors();
        if l.panels.screen != crate::panels::Screen::Conversation {
            l.panels.screen = crate::panels::Screen::Conversation;
        }
        index
    };
    open_session(s, index, orientation);
}

pub fn open_checkout(s: &mut Workspace, path: &str, orientation: SplitOrientation) {
    let index = {
        let l = live_mut(s);
        let Some(index) = l.app.checkout_session_for_split(path) else {
            return;
        };
        l.sync_editors();
        if l.panels.screen != crate::panels::Screen::Conversation {
            l.panels.screen = crate::panels::Screen::Conversation;
        }
        index
    };
    open_session(s, index, orientation);
}

pub fn open_review(s: &mut Workspace, project: String, url: String) {
    let l = live_mut(s);
    l.panels.review = Some((project.clone(), url.clone()));
    l.split.push_pane(
        SecondaryPane::Review(project, url),
        SplitOrientation::Vertical,
    );
}

pub fn toggle_orientation(s: &mut Workspace) {
    let l = live_mut(s);
    if !l.split.is_open() {
        return;
    }
    l.split.orientation = match l.split.orientation {
        SplitOrientation::Vertical => SplitOrientation::Horizontal,
        SplitOrientation::Horizontal => SplitOrientation::Vertical,
    };
}

/// Rotates the split deck forward (next off-screen pane rotates into view; oldest visible
/// session slides off-screen). If only two sessions are open, swaps them.
pub fn rotate_forward(s: &mut Workspace) {
    let l = live_mut(s);
    if l.split.deck.is_empty() {
        return;
    }
    if l.split.deck.len() == 1 {
        swap(s);
        return;
    }
    let visible_sec = l.split.deck.remove(0);
    let next_sec = l.split.deck.remove(0);
    let prev_primary = l.app.selected;
    if let SecondaryPane::Session(sec_idx) = visible_sec {
        l.app.select(sec_idx);
        if let Some(prev) = prev_primary.filter(|&p| p != sec_idx) {
            l.split.deck.push(SecondaryPane::Session(prev));
        }
    } else {
        l.split.deck.push(visible_sec);
    }
    l.split.deck.insert(0, next_sec);
    let active = l.split.active;
    focus_pane(s, active);
}

/// Rotates the split deck backward (most recently off-screen pane rotates back into view).
pub fn rotate_backward(s: &mut Workspace) {
    let l = live_mut(s);
    if l.split.deck.is_empty() {
        return;
    }
    if l.split.deck.len() == 1 {
        swap(s);
        return;
    }
    let Some(last) = l.split.deck.pop() else {
        return;
    };
    let prev_primary = l.app.selected;
    if let SecondaryPane::Session(last_idx) = last {
        l.app.select(last_idx);
        if let Some(prev) = prev_primary.filter(|&p| p != last_idx) {
            l.split.deck.insert(0, SecondaryPane::Session(prev));
        }
    } else {
        l.split.deck.insert(0, last);
    }
    let active = l.split.active;
    focus_pane(s, active);
}

/// Brings a specific off-screen session from the deck into the visible secondary slot and focuses it.
pub fn bring_to_view(s: &mut Workspace, index: usize) {
    let l = live_mut(s);
    if index >= l.app.sessions.len() {
        return;
    }
    if l.app.selected == Some(index) {
        focus_pane(s, ActivePane::Primary);
        return;
    }
    if l.split.deck.first() == Some(&SecondaryPane::Session(index)) {
        focus_pane(s, ActivePane::Secondary);
        return;
    }
    let orientation = l.split.orientation;
    open_session(s, index, orientation);
}

pub fn swap(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(SecondaryPane::Session(secondary)) = l.split.deck.first().cloned() else {
        return;
    };
    let primary = l.app.selected;
    l.split.deck.remove(0);
    l.app.select(secondary);
    if let Some(prev) = primary.filter(|&p| p != secondary) {
        l.split.deck.insert(0, SecondaryPane::Session(prev));
        focus_pane(s, ActivePane::Primary);
    } else if l.split.deck.is_empty() {
        close(s);
    }
}

pub fn close(s: &mut Workspace) {
    live_mut(s).split.close_visible();
    let active = if live(s).split.is_open() {
        live(s).split.active
    } else {
        ActivePane::Primary
    };
    focus_pane(s, active);
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
            if let Some(i) = l.app.selected {
                if let Some(project) = l.app.sessions.get(i).map(|s| s.scope.project.clone()) {
                    l.app.focus_project_context(&project);
                }
                if let Some(draft) = l.drafts.get_mut(i) {
                    draft.focused = true;
                    l.focus_key = Some(format!("live-draft-{i}"));
                }
            }
        }
        ActivePane::Secondary => {
            if let Some(SecondaryPane::Session(i)) = l.split.visible_secondary().cloned() {
                if let Some(project) = l.app.sessions.get(i).map(|s| s.scope.project.clone()) {
                    l.app.focus_project_context(&project);
                }
                if let Some(draft) = l.drafts.get_mut(i) {
                    draft.focused = true;
                    l.focus_key = Some(format!("live-draft-{i}"));
                }
            }
        }
    }
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    let Some(l) = s.live.as_ref() else {
        return false;
    };
    if !l.split.is_open() {
        return false;
    }
    if ev.is_command(']') && ev.mods.alt {
        rotate_forward(s);
        return true;
    }
    if ev.is_command('[') && ev.mods.alt {
        rotate_backward(s);
        return true;
    }
    if !l.split.is_visible(l.window_width.get()) {
        return false;
    }
    if ev.mods.cmd && ev.mods.alt && matches!(ev.key, Key::Right | Key::Down) {
        focus_pane(s, ActivePane::Secondary);
        return true;
    }
    if ev.mods.cmd && ev.mods.alt && matches!(ev.key, Key::Left | Key::Up) {
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
        && let Some(SecondaryPane::Session(index)) = l.split.visible_secondary().cloned()
        && l.app.sessions.get(index).is_some_and(|s| s.working())
    {
        live_mut(s).app.cancel(index);
        return true;
    }
    false
}
