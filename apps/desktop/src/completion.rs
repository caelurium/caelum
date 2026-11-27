//! Caret-aware native command/file/PR completion. Requests and captured context are core-owned.
mod commands;
mod owner;
use crate::{
    Workspace,
    live::{live, live_mut},
    ui,
};
use caelum_core::application::{Completion, CompletionKind, Provider, Task};
use owner::EditorOwner;
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, icon, row, text};
use rune_kit::{Kit, Placement, ScrollBar, popover};

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
        Some(&self.owner) == EditorOwner::current(l).as_ref()
            && self.owner.editor(l).is_some_and(|editor| {
                editor.focused
                    && editor.preedit().is_none()
                    && self.text == editor.text
                    && self.anchor == editor.anchor
                    && self.focus == editor.focus
            })
    }
}

pub fn sync(l: &mut crate::live::Live) {
    sync_templates(l);
    if let Phase::Applying { request, capture } = &l.completion.phase {
        if !l.app.mention_pending(*request) {
            l.completion.phase = Phase::Available;
        } else if !capture.matches(l) || l.panels.overlay().is_some() || l.models.is_open() {
            cancel(l);
        }
    }
}

pub(crate) fn sync_templates(l: &mut crate::live::Live) {
    let target = EditorOwner::current(l)
        .filter(|owner| {
            owner.editor(l).is_some_and(|e| {
                let text = e.text.trim_start();
                text.starts_with('/') && !text.starts_with("//")
            })
        })
        .and_then(|owner| owner.target(l));
    l.app.show_templates(target);
}

pub fn cancel(l: &mut crate::live::Live) {
    if let Phase::Applying { capture, .. } = std::mem::take(&mut l.completion.phase)
        && let Some(i) = capture.owner.index()
    {
        l.app.cancel_mentions(i);
    }
}

#[derive(Clone)]
struct Choice {
    title: String,
    args: String,
    detail: String,
    value: String,
    project: String,
    badge: Option<&'static str>,
}

pub fn editor(s: &Workspace) -> Option<&TextEdit> {
    let l = live(s);
    EditorOwner::current(l)?.editor(l)
}

pub fn token(s: &Workspace) -> Option<Completion> {
    let l = live(s);
    let owner = EditorOwner::current(l)?;
    let editor = owner.editor(l)?;
    if !editor.focused
        || editor.preedit().is_some()
        || l.panels.overlay().is_some()
        || l.models.is_open()
    {
        return None;
    }
    if matches!(&l.completion.phase, Phase::Dismissed(capture) if capture.matches(l)) {
        return None;
    }
    Completion::at(&editor.text, editor.anchor, editor.focus)
        .filter(|t| owner.index().is_some() || t.kind == CompletionKind::Command)
}

fn choices(s: &Workspace, token: &Completion) -> Vec<Choice> {
    let l = live(s);
    let Some(owner) = EditorOwner::current(l) else {
        return Vec::new();
    };
    let query = token.query.to_lowercase();
    if token.kind == CompletionKind::Command {
        return commands::choices(l, &owner, &query);
    }
    let Some(index) = l.app.selected else {
        return Vec::new();
    };
    let session = &l.app.sessions[index];
    let project = session.scope.project.clone();
    match token.kind {
        CompletionKind::Command => unreachable!(),
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
                        args: String::new(),
                        detail: "Attach file snapshot".into(),
                        value: hit.path.clone(),
                        project: project.clone(),
                        badge: None,
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
                    args: String::new(),
                    detail: pr.title.clone(),
                    value: pr.url.clone(),
                    project: p.clone(),
                    badge: None,
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
        let index = owner.index();
        let capture = Capture::new(owner, editor);
        if let Some(i) = index {
            l.app.cancel_mentions(i);
        }
        l.completion.phase = Phase::Dismissed(capture);
    }
}

fn choose(s: &mut Workspace, choice: Choice, token: Completion, execute: bool, tx: &mut Tx) {
    if live(s).panels.overlay().is_some()
        || editor(s).is_none_or(|editor| {
            editor.preedit().is_some()
                || Completion::at(&editor.text, editor.anchor, editor.focus).as_ref()
                    != Some(&token)
        })
    {
        return;
    }
    let Some(owner) = EditorOwner::current(live(s)) else {
        return;
    };
    if owner
        .index()
        .is_some_and(|i| live(s).app.sessions[i].attaching > 0)
    {
        return;
    }
    if token.kind == CompletionKind::Command {
        let l = live_mut(s);
        let (text, caret) = token.replace(
            &owner.editor(l).unwrap().text,
            &format!("/{}", choice.value),
        );
        owner.replace(l, text, caret);
        dismiss(s);
        if execute {
            if let EditorOwner::Boat { thread, .. } = owner {
                crate::remote::send_draft(s, tx, &thread);
            } else {
                s.send(tx);
            }
        }
    } else {
        let Some(index) = owner.index() else { return };
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
    let Some(editor) = editor(s) else {
        return false;
    };
    if !editor.focused || editor.preedit().is_some() || live(s).panels.overlay().is_some() {
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
    let Some(EditorOwner::Composer(index)) = EditorOwner::current(live(s)) else {
        return false;
    };
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
    let anchor = if crate::home::is_home(s) {
        "home-editor"
    } else {
        "composer-editor"
    };
    let Some([_, _, width, _]) = rune::memo::window_layout(anchor) else {
        return;
    };

    layers.push(popover::anchored(
        k,
        popover::raised(k, panel(s, k).w(width), ui::POPUP_RADIUS),
        anchor,
        Placement::TopStart,
        ui::POPUP_OFFSET,
    ));
}

pub fn view(s: &Workspace, k: &Kit) -> El {
    if token(s).is_none() {
        return col();
    }
    panel(s, k)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .radius(ui::POPUP_RADIUS)
}

fn panel(s: &Workspace, k: &Kit) -> El {
    let Some(token) = token(s) else {
        return col();
    };
    let items = choices(s, &token);
    let count = items.len();
    let selected = live(s).completion.active.min(count.saturating_sub(1));
    let (heading, hint) = match token.kind {
        CompletionKind::Command => ("Slash commands", "↑↓ Navigate · Tab Complete · ↵ Run"),
        CompletionKind::File => ("Attach file", "↑↓ Navigate · Tab / ↵ Attach"),
        CompletionKind::PullRequest => ("Attach pull request", "↑↓ Navigate · Tab / ↵ Attach"),
    };
    let count_label = match (token.kind, count) {
        (_, 0) => None,
        (CompletionKind::Command, 1) => Some("1 command".into()),
        (CompletionKind::Command, n) => Some(format!("{n} commands")),
        (_, 1) => Some("1 match".into()),
        (_, n) => Some(format!("{n} matches")),
    };
    let mut content = col().w_full().min_h(0.0).gap(4.0).child(ui::popup_header(
        heading,
        None,
        count_label.map(|label| ui::caption(label, k)),
        k,
    ));
    let l = live(s);
    let owner = EditorOwner::current(l).unwrap();
    let session = owner.index().map(|i| &l.app.sessions[i]);
    let busy = session.is_some_and(|s| s.attaching > 0)
        || token.kind == CompletionKind::File
            && l.app.services.busy(&Task::Completions {
                project: session.unwrap().scope.project.clone(),
                query: token.query.clone(),
            });
    if busy {
        content = content.child(ui::skeleton("Loading context", 1, k).px(8.0));
    }
    if token.kind == CompletionKind::Command
        && matches!(
            l.app.templates,
            caelum_core::templates::Catalog::Loading { .. }
        )
    {
        content = content.child(ui::caption("Loading prompt templates…", k).px(8.0));
    }
    if let Some(error) = session
        .and_then(|s| {
            s.error.as_ref().or_else(|| {
                l.app
                    .services
                    .checkouts
                    .get(&s.scope.project)?
                    .error
                    .as_ref()
            })
        })
        .or_else(|| match &l.app.templates {
            caelum_core::templates::Catalog::Failed { error, .. }
                if token.kind == CompletionKind::Command =>
            {
                Some(error)
            }
            _ => None,
        })
    {
        content = content.child(ui::caption(error, k).color(k.c.danger.fg).px(8.0));
    }
    if items.is_empty() && !busy {
        let empty_title = match token.kind {
            CompletionKind::Command => "No matching commands",
            CompletionKind::File => "No matching files",
            CompletionKind::PullRequest => "No matching pull requests",
        };
        content = content.child(
            col()
                .w_full()
                .gap(2.0)
                .px(8.0)
                .py(10.0)
                .child(text(empty_title).font(k.ty.ui_medium).color(k.c.text_base))
                .child(ui::caption(
                    "Try a shorter query or press Esc to dismiss.",
                    k,
                )),
        );
    }
    let kit = k.clone();
    let options = col()
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count,
            36.0,
            |_: &Workspace, i| format!("completion-{i}").into(),
            move |_: &Workspace, i| {
                let choice = items[i].clone();
                let t = token.clone();
                let captured = owner.clone();
                completion_row(&choice, token.kind, i == selected, &kit)
                    .disabled(busy)
                    .on_press_tx(move |s: &mut Workspace, tx| {
                        if EditorOwner::current(live(s)).as_ref() == Some(&captured) {
                            choose(s, choice.clone(), t.clone(), false, tx)
                        }
                    })
                    .w_full()
                    .h(36.0)
            },
        );
    content
        .child(
            col()
                .w_full()
                .shrink()
                .min_h(0.0)
                .max_h(210.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(options)
                .scroll_bar(k),
        )
        .child(ui::divider(k))
        .child(
            row()
                .w_full()
                .px(8.0)
                .pt(3.0)
                .pb(1.0)
                .gap(8.0)
                .center_y()
                .justify_between()
                .child(ui::caption(hint, k).truncate().grow(1.0).min_w(0.0))
                .child(ui::caption("Esc", k)),
        )
        .padding(6.0)
        .keeps_text_selection()
        .on_press(|_: &mut Workspace| {})
        .key("completion-menu")
}

fn command_args(command: caelum_core::application::commands::LocalCommand) -> &'static str {
    use caelum_core::application::commands::LocalCommand;
    match command {
        LocalCommand::Effort => "[level]",
        LocalCommand::Rename => "[title]",
        LocalCommand::Export => "[md|json]",
        LocalCommand::SideQuestion => "[question]",
        _ => "",
    }
}

fn completion_row(item: &Choice, kind: CompletionKind, selected: bool, k: &Kit) -> El {
    let leading = match kind {
        CompletionKind::Command => None,
        CompletionKind::File => Some(&ui::FILE),
        CompletionKind::PullRequest => Some(&ui::BRANCH),
    };
    let icon_color = if selected {
        k.c.text_accent
    } else {
        k.c.icon_muted
    };
    let body = match kind {
        CompletionKind::Command => row()
            .grow(1.0)
            .min_w(0.0)
            .gap(10.0)
            .center_y()
            .child(
                row()
                    .min_w(136.0)
                    .gap(5.0)
                    .center_y()
                    .child(text(&item.title).font(k.ty.mono).color(if selected {
                        k.c.text_accent
                    } else {
                        k.c.text_base
                    }))
                    .children(
                        (!item.args.is_empty())
                            .then(|| text(&item.args).font(k.ty.mono).color(k.c.text_faint)),
                    ),
            )
            .children(
                (!item.detail.is_empty())
                    .then(|| ui::caption(&item.detail, k).truncate().grow(1.0).min_w(0.0)),
            )
            .children(item.badge.map(|badge| {
                text(badge)
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .px(6.0)
                    .py(2.0)
                    .radius(4.0)
                    .fill(k.c.layer_02)
            })),
        CompletionKind::File => {
            let (parent, name) = item
                .title
                .rsplit_once('/')
                .unwrap_or(("", item.title.as_str()));
            row()
                .grow(1.0)
                .min_w(0.0)
                .gap(8.0)
                .center_y()
                .child(
                    text(name)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .truncate(),
                )
                .children(
                    (!parent.is_empty())
                        .then(|| ui::caption(parent, k).truncate().grow(1.0).min_w(0.0)),
                )
        }
        CompletionKind::PullRequest => row()
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .center_y()
            .child(text(&item.title).font(k.ty.mono).color(if selected {
                k.c.text_accent
            } else {
                k.c.text_muted
            }))
            .child(
                text(&item.detail)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            ),
    };
    let description = if item.detail.is_empty() {
        item.title.as_str()
    } else {
        item.detail.as_str()
    };
    k.interactive(
        row()
            .w_full()
            .min_h(34.0)
            .px(8.0)
            .py(6.0)
            .gap(8.0)
            .center_y()
            .radius(6.0)
            .fill(if selected {
                k.c.hover
            } else {
                rune::Color::TRANSPARENT
            })
            .children(leading.map(|glyph| icon(glyph, 13.0).color(icon_color).a11y_hidden()))
            .child(body)
            .child(
                text("↵")
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .opacity(if selected { 1.0 } else { 0.0 })
                    .a11y_hidden(),
            ),
    )
    .role("button")
    .label(&item.title)
    .description(description)
    .focusable(true)
    .focus_ring(1.5, 0.0, k.c.border_focus)
}
