//! Split-pane workspace layout: view two sessions side-by-side or a session beside a PR review.

use rune::{El, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, row, text, text_input};
use rune_kit::{ButtonVariant, Kit, Markdown, glyphs};

use crate::{
    SEND, STOP, Workspace,
    context::EditMenu,
    live::{live, live_mut},
    quiet_button,
    ui::{self, Button},
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
        .child(
            rune::el()
                .w_full()
                .h(2.0)
                .fill(if primary_active {
                    k.c.text_accent
                } else {
                    rune::Color::TRANSPARENT
                })
                .a11y_hidden(),
        )
        .child(primary.grow(1.0).min_h(0.0));

    let right = secondary_pane(s, k, secondary, !primary_active, pane_border, layers)
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

fn secondary_pane(
    s: &Workspace,
    k: &Kit,
    secondary: SecondaryPane,
    active: bool,
    pane_border: rune::Color,
    layers: &mut Vec<El>,
) -> El {
    let title = match &secondary {
        SecondaryPane::Session(index) => live(s)
            .app
            .sessions
            .get(*index)
            .map(|sess| sess.title.clone())
            .unwrap_or_else(|| "Session".into()),
        SecondaryPane::Review(_, url) => format!(
            "PR Review · {}",
            url.rsplit('/').next().unwrap_or(url.as_str())
        ),
    };

    let header = row()
        .w_full()
        .h(38.0)
        .px(14.0)
        .gap(8.0)
        .center_y()
        .child(
            text(title)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(ui::caption(if active { "Active" } else { "Split" }, k))
        .child(
            quiet_button(&glyphs::CLOSE, "Close split pane")
                .on_press(close)
                .build(k)
                .key("split-close"),
        );

    let content = match secondary {
        SecondaryPane::Session(index) => secondary_session(s, k, index),
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
        .child(
            rune::el()
                .w_full()
                .h(2.0)
                .fill(if active {
                    k.c.text_accent
                } else {
                    rune::Color::TRANSPARENT
                })
                .a11y_hidden(),
        )
        .child(header)
        .child(rune::el().w_full().h(1.0).fill(pane_border).a11y_hidden())
        .child(content.grow(1.0).min_h(0.0))
}

fn secondary_session(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let Some(session) = l.app.sessions.get(index) else {
        return ui::empty(
            "Session unavailable",
            "Close the split pane or select another session.",
            k,
        );
    };

    let mut messages = col().w_full().px(18.0).py(16.0).gap(16.0);
    if session.conversation.entries.is_empty() {
        messages = messages.child(ui::empty(
            "No messages yet",
            "Send a message below in this split session.",
            k,
        ));
    } else {
        for entry in session
            .conversation
            .entries
            .iter()
            .filter(|e| caelum_core::application::presentation::entry_visible(e))
        {
            match entry.kind() {
                caelum_core::application::EntryKind::User => {
                    messages = messages.child(ui::user_message(&entry.text, k));
                }
                caelum_core::application::EntryKind::Assistant => {
                    messages = messages.child(Markdown::new(&entry.text).selectable(true).build(k));
                }
                _ => {}
            }
        }
    }

    let scroll = col()
        .key(format!("split-transcript-{index}"))
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: true })
        .child(messages);

    let draft = &l.drafts[index];
    let working = session.working();
    let input = text_input(draft, TextStyle::new(13.5, 400.0).lh(21.0))
        .label("Split message")
        .placeholder("Message split session…", k.c.text_muted)
        .color(k.c.text_base)
        .selection_color(k.selection())
        .w_full()
        .edit_menu(format!("split-draft-{index}"), move |s| {
            live_mut(s).drafts.get_mut(index)
        })
        .on_input_key(crate::on_key)
        .on_submit(move |s: &mut Workspace, _tx| send_secondary(s, index));

    let action = if working {
        Button::new("Stop")
            .icon(&STOP)
            .variant(ButtonVariant::Danger)
            .on_press(move |s: &mut Workspace| live_mut(s).app.cancel(index))
            .build(k)
            .key(format!("split-stop-{index}"))
    } else {
        Button::new("Send")
            .icon(&SEND)
            .variant(ButtonVariant::Contrast)
            .disabled(!session.can_send() || draft.text.trim().is_empty())
            .on_press(move |s: &mut Workspace| send_secondary(s, index))
            .build(k)
            .key(format!("split-send-{index}"))
    };

    let composer = row()
        .w_full()
        .px(14.0)
        .py(10.0)
        .gap(8.0)
        .center_y()
        .border(1.0, k.c.border_muted)
        .child(input.grow(1.0).min_w(0.0))
        .child(action);

    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(scroll)
        .child(composer)
}

fn send_secondary(s: &mut Workspace, index: usize) {
    let l = live_mut(s);
    if index >= l.drafts.len() {
        return;
    }
    let text = l.drafts[index].text.clone();
    if text.trim().is_empty() {
        return;
    }
    if l.app.send(index, text) {
        l.drafts[index].clear();
    }
}
