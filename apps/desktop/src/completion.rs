//! Caret-aware native command/file/PR completion. Requests and captured context are core-owned.
use crate::{
    Workspace,
    live::{live, live_mut},
    ui,
};
use caelum_core::application::{Completion, CompletionKind, Provider, Task};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col};
use rune_kit::{Kit, Placement, popover};

#[derive(Default)]
pub struct Menu {
    pub active: usize,
    phase: Phase,
}

#[derive(Default)]
enum Phase {
    #[default]
    Available,
    Applying {
        request: u64,
        capture: Capture,
    },
    Dismissed(Capture),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditorOwner {
    Composer(usize),
    SideQuestion(usize),
}

impl EditorOwner {
    fn index(self) -> usize {
        match self {
            Self::Composer(index) | Self::SideQuestion(index) => index,
        }
    }

    fn current(l: &crate::live::Live) -> Option<Self> {
        let index = l.app.selected?;
        Some(if l.commands.side_owner == Some(index) {
            Self::SideQuestion(index)
        } else {
            Self::Composer(index)
        })
    }

    fn editor(self, l: &crate::live::Live) -> Option<&TextEdit> {
        match self {
            Self::Composer(index) => l.drafts.get(index),
            Self::SideQuestion(index) => {
                (l.commands.side_owner == Some(index)).then_some(&l.commands.side_editor)
            }
        }
    }
}

struct Capture {
    owner: EditorOwner,
    text: String,
    anchor: usize,
    focus: usize,
}

impl Capture {
    fn new(owner: EditorOwner, editor: &TextEdit) -> Self {
        Self {
            owner,
            text: editor.text.clone(),
            anchor: editor.anchor,
            focus: editor.focus,
        }
    }

    fn matches(&self, l: &crate::live::Live) -> bool {
        Some(self.owner) == EditorOwner::current(l)
            && self.owner.editor(l).is_some_and(|editor| {
                editor.focused
                    && editor.preedit.is_none()
                    && self.text == editor.text
                    && self.anchor == editor.anchor
                    && self.focus == editor.focus
            })
    }
}

pub fn sync(l: &mut crate::live::Live) {
    if let Phase::Applying { request, capture } = &l.completion.phase {
        if !l.app.mention_pending(*request) {
            l.completion.phase = Phase::Available;
        } else if !capture.matches(l) || l.panels.overlay().is_some() || l.models.is_open() {
            cancel(l);
        }
    }
}

pub fn cancel(l: &mut crate::live::Live) {
    if let Phase::Applying { capture, .. } = std::mem::take(&mut l.completion.phase) {
        l.app.cancel_mentions(capture.owner.index());
    }
}

#[derive(Clone)]
struct Choice {
    title: String,
    detail: String,
    value: String,
    project: String,
}

pub fn editor(s: &Workspace) -> Option<&TextEdit> {
    let l = live(s);
    let index = l.app.selected?;
    if l.commands.side_owner == Some(index) {
        Some(&l.commands.side_editor)
    } else {
        l.drafts.get(index)
    }
}

pub fn token(s: &Workspace) -> Option<Completion> {
    let l = live(s);
    let index = l.app.selected?;
    let editor = editor(s)?;
    if !editor.focused
        || !l.app.sessions[index].scope.provider_available()
        || l.panels.screen != crate::panels::Screen::Conversation
        || editor.preedit.is_some()
        || l.panels.overlay().is_some()
        || l.models.is_open()
    {
        return None;
    }
    if matches!(&l.completion.phase, Phase::Dismissed(capture) if capture.matches(l)) {
        return None;
    }
    Completion::at(&editor.text, editor.anchor, editor.focus)
}

fn choices(s: &Workspace, token: &Completion) -> Vec<Choice> {
    let l = live(s);
    let Some(index) = l.app.selected else {
        return Vec::new();
    };
    let session = &l.app.sessions[index];
    let project = session.scope.project.clone();
    let query = token.query.to_lowercase();
    match token.kind {
        CompletionKind::Command if l.commands.side_owner.is_some() => Vec::new(),
        CompletionKind::Command => {
            let mut choices: Vec<_> = caelum_core::application::commands::COMMANDS
                .iter()
                .filter(|(_, _, c)| c.available(session.scope.provider))
                .map(|(n, d, _)| (n.to_string(), d.to_string()))
                .collect();
            if session.scope.provider == Provider::OpenCode
                && let Some(commands) = l.app.provider_commands.get(&session.scope)
            {
                for c in commands {
                    if !choices.iter().any(|(n, _)| n == &c.name) {
                        choices.push((c.name.clone(), c.description.clone().unwrap_or_default()));
                    }
                }
            }
            choices
                .into_iter()
                .filter(|(n, d)| format!("{n} {d}").to_lowercase().contains(&query))
                .map(|(name, detail)| Choice {
                    title: format!("/{name}"),
                    value: name,
                    detail,
                    project: project.clone(),
                })
                .collect()
        }
        CompletionKind::File => l
            .app
            .services
            .checkouts
            .get(&project)
            .filter(|c| c.completion_query == token.query)
            .and_then(|c| c.completions.as_ref())
            .map(|r| {
                r.matches
                    .iter()
                    .map(|hit| Choice {
                        title: hit.path.clone(),
                        detail: "Attach file snapshot".into(),
                        value: hit.path.clone(),
                        project: project.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        CompletionKind::PullRequest => {
            let mut seen = std::collections::HashSet::new();
            let mut matches: Vec<_> = l
                .app
                .services
                .checkouts
                .iter()
                .filter(|(p, _)| l.app.group_path(p) == l.app.group_path(&project))
                .flat_map(|(p, c)| c.pull_requests.iter().map(move |pr| (p, pr)))
                .filter(|(_, pr)| {
                    format!("{} {}", pr.number, pr.title)
                        .to_lowercase()
                        .contains(&query)
                        && seen.insert(pr.url.clone())
                })
                .collect();
            matches.sort_by_key(|(_, pr)| std::cmp::Reverse(pr.number));
            matches
                .into_iter()
                .map(|(p, pr)| Choice {
                    title: format!("#{}", pr.number),
                    detail: pr.title.clone(),
                    value: pr.url.clone(),
                    project: p.clone(),
                })
                .collect()
        }
    }
}

pub fn changed(s: &mut Workspace, _: &mut Tx) {
    crate::latency::edit(live(s));
    let l = live_mut(s);
    l.completion.active = 0;
    if matches!(l.completion.phase, Phase::Dismissed(_)) {
        l.completion.phase = Phase::Available;
    }
    if let Some(index) = l.app.selected {
        l.app.sessions[index].completion_generation += 1;
        if l.commands.side_owner == Some(index) {
            l.app.sessions[index]
                .side_draft
                .clone_from(&l.commands.side_editor.text);
        } else {
            if !l.drafts[index].text.starts_with('/') {
                l.commands.previous_drafts[index].clone_from(&l.drafts[index].text);
            }
            l.app.sessions[index]
                .draft
                .clone_from(&l.drafts[index].text);
        }
    }
}

fn dismiss(s: &mut Workspace) {
    let l = live_mut(s);
    if let Some(owner) = EditorOwner::current(l) {
        let Some(editor) = owner.editor(l) else {
            return;
        };
        let capture = Capture::new(owner, editor);
        l.app.cancel_mentions(owner.index());
        l.completion.phase = Phase::Dismissed(capture);
    }
}

fn choose(s: &mut Workspace, choice: Choice, token: Completion, execute: bool, tx: &mut Tx) {
    if live(s).panels.overlay().is_some()
        || editor(s).is_none_or(|editor| {
            editor.preedit.is_some()
                || Completion::at(&editor.text, editor.anchor, editor.focus).as_ref()
                    != Some(&token)
        })
    {
        return;
    }
    let Some(index) = live(s).app.selected else {
        return;
    };
    if live(s).app.sessions[index].attaching > 0 {
        return;
    }
    if token.kind == CompletionKind::Command {
        let l = live_mut(s);
        let (text, caret) = token.replace(&l.drafts[index].text, &format!("/{}", choice.value));
        l.drafts[index].replace_text(text);
        l.drafts[index].set_caret(caret);
        l.drafts[index].focused = true;
        dismiss(s);
        if execute {
            s.send(tx);
        }
    } else {
        let replacement = if token.kind == CompletionKind::File {
            format!("@{}", choice.value)
        } else {
            choice.title
        };
        let l = live_mut(s);
        let (capture, request) = if l.commands.side_owner == Some(index) {
            l.commands.side_editor.focused = true;
            let editor = &l.commands.side_editor;
            let capture = Capture::new(EditorOwner::SideQuestion(index), editor);
            l.app.sessions[index]
                .side_draft
                .clone_from(&l.commands.side_editor.text);
            let request = l.app.complete_side_mention(
                index,
                token,
                choice.value,
                choice.project,
                replacement,
            );
            (capture, request)
        } else {
            l.drafts[index].focused = true;
            let editor = &l.drafts[index];
            let capture = Capture::new(EditorOwner::Composer(index), editor);
            l.app.sessions[index]
                .draft
                .clone_from(&l.drafts[index].text);
            let request =
                l.app
                    .complete_mention(index, token, choice.value, choice.project, replacement);
            (capture, request)
        };
        if let Some(request) = request {
            l.completion.phase = Phase::Applying { request, capture };
        }
    }
}

pub fn on_key(s: &mut Workspace, event: &KeyEvent, tx: &mut Tx) -> bool {
    if s.live.is_none() {
        return false;
    }
    let Some(index) = live(s).app.selected else {
        return false;
    };
    let Some(editor) = editor(s) else {
        return false;
    };
    if !editor.focused || editor.preedit.is_some() || live(s).panels.overlay().is_some() {
        return false;
    }
    let active = token(s);
    if let Some(token) = active
        && !event.mods.shift
        && !event.mods.cmd
        && !event.mods.ctrl
        && !event.mods.alt
    {
        let items = choices(s, &token);
        let selected = live(s).completion.active.min(items.len().saturating_sub(1));
        match event.key {
            Key::Escape => {
                dismiss(s);
                return true;
            }
            Key::Down => {
                live_mut(s).completion.active = (selected + 1).min(items.len().saturating_sub(1));
                tx.scroll_into_view(format!("completion-{}", live(s).completion.active));
                return true;
            }
            Key::Up => {
                live_mut(s).completion.active = selected.saturating_sub(1);
                tx.scroll_into_view(format!("completion-{}", live(s).completion.active));
                return true;
            }
            Key::Enter | Key::Tab => {
                if let Some(choice) = items.get(selected) {
                    choose(s, choice.clone(), token, event.key == Key::Enter, tx);
                    return true;
                }
                return token.kind != CompletionKind::Command;
            }
            _ => {}
        }
    }
    if live(s).commands.side_owner.is_some() {
        return false;
    }
    if !event.mods.cmd
        && !event.mods.ctrl
        && !event.mods.alt
        && event.key == Key::Up
        && !event.mods.shift
        && live(s).drafts[index].text.is_empty()
    {
        let last = live(s).app.sessions[index]
            .conversation
            .entries
            .iter()
            .rev()
            .find(|e| e.kind() == caelum_core::application::EntryKind::User)
            .map(|e| {
                caelum_core::application::split_context(&e.text)
                    .0
                    .to_owned()
            });
        if let Some(text) = last {
            let draft = &mut live_mut(s).drafts[index];
            draft.replace_text(caelum_core::application::presentation::visible_text(&text));
            draft.set_caret(draft.text.len());
            return true;
        }
    }
    if event.key == Key::Tab
        && event.mods.shift
        && !event.mods.cmd
        && !event.mods.ctrl
        && !event.mods.alt
    {
        let l = live_mut(s);
        let session = &mut l.app.sessions[index];
        if session.scope.provider == Provider::OpenCode
            && !session.archived()
            && !session.forgotten()
            && !session.archiving()
            && let Some(choices) = l.app.choices.get(&session.scope)
            && !choices.agents.is_empty()
        {
            let next = choices
                .agents
                .iter()
                .position(|a| Some(&a.id) == session.options.agent.as_ref())
                .map_or(0, |i| (i + 1) % choices.agents.len());
            session.options.agent = Some(choices.agents[next].id.clone());
            l.app.options_changed(index);
            l.focus();
            return true;
        }
    }

    let _ = live_mut(s);
    false
}

pub fn overlay(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    if token(s).is_none() {
        return;
    }
    let Some([_, _, width, _]) = rune::memo::window_layout("composer-editor") else {
        return;
    };

    layers.push(popover::anchored(
        k,
        popover::raised(k, view(s, k).w(width), 8.0),
        "composer-editor",
        Placement::TopStart,
        4.0,
    ));
}

pub fn view(s: &Workspace, k: &Kit) -> El {
    let Some(token) = token(s) else {
        return col();
    };
    let items = choices(s, &token);
    let selected = live(s).completion.active.min(items.len().saturating_sub(1));
    let title = match token.kind {
        CompletionKind::Command => "Commands · ↑↓ Navigate · Tab Complete · Enter Run",
        CompletionKind::File => "Files · Tab / Enter Attach",
        CompletionKind::PullRequest => "Session pull requests · Tab / Enter Attach",
    };
    let mut content = col()
        .w_full()
        .min_h(0.0)
        .gap(3.0)
        .child(ui::caption(title, k).truncate());
    let l = live(s);
    let index = l.app.selected.unwrap();
    let project = &l.app.sessions[index].scope.project;
    let busy = l.app.sessions[index].attaching > 0
        || token.kind == CompletionKind::File
            && l.app.services.busy(&Task::Completions {
                project: project.clone(),
                query: token.query.clone(),
            });
    if busy {
        content = content.child(ui::skeleton("Loading context", 1, k));
    }
    if let Some(error) = l.app.sessions[index].error.as_ref().or_else(|| {
        l.app
            .services
            .checkouts
            .get(project)
            .and_then(|c| c.error.as_ref())
    }) {
        content = content.child(ui::caption(error, k).color(k.c.danger.fg));
    }
    let mut options = col().w_full().gap(2.0);
    if items.is_empty() && !busy {
        content = content
            .child(ui::caption("No matches. Keep typing to refine your search.", k).padding(10.0));
    }
    for (i, item) in items.into_iter().enumerate() {
        let t = token.clone();
        let choice = item.clone();
        options = options.child(
            ui::choice(&item.title, &item.detail, i == selected, k)
                .disabled(busy)
                .on_press_tx(move |s: &mut Workspace, tx| {
                    choose(s, choice.clone(), t.clone(), false, tx)
                })
                .w_full()
                .key(format!("completion-{i}")),
        );
    }
    content
        .child(
            col()
                .w_full()
                .shrink()
                .min_h(0.0)
                .max_h(170.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(options),
        )
        .padding(8.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .radius(8.0)
        .keeps_text_selection()
        .on_press(|_: &mut Workspace| {})
        .key("completion-menu")
}
