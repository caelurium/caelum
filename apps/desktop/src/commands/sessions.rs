use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    panels::Screen,
    ui::{self, Button},
};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row};
use rune_kit::{ButtonVariant, Kit};

fn session_matches(s: &Workspace, index: usize) -> Vec<usize> {
    let l = live(s);
    let current = &l.app.sessions[index];
    let query = l.commands.sessions_search.text.to_lowercase();
    let project_group = l.app.group_path(&current.scope.project);
    let matches: Vec<_> = l
        .app
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, session)| {
            !session.forgotten()
                && session.remote_id.is_some()
                && session.scope.provider == current.scope.provider
                && l.app.group_path(&session.scope.project) == project_group
                && format!(
                    "{} {} {} {}",
                    session.title,
                    session.scope.provider.as_str(),
                    session.scope.project,
                    session.remote_id.as_deref().unwrap_or_default()
                )
                .to_lowercase()
                .contains(&query)
        })
        .map(|(index, _)| index)
        .collect();
    matches
}

pub(super) fn sessions(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let owner = &l.app.sessions[index];
    let project = owner.scope.project.clone();
    let scope = owner.scope.clone();
    let matches = session_matches(s, index);
    let selected = l.commands.sessions_selected.clone();
    let rows = col()
        .w_full()
        .gap(3.0)
        .key("provider-sessions")
        .role("list")
        .focus_group();
    let mut rows = rows;
    for session_index in &matches {
        let session_index = *session_index;
        let session = &l.app.sessions[session_index];
        let id = session.remote_id.as_deref().unwrap_or_default().to_owned();
        let title = if session.title.trim().is_empty() {
            "Untitled session"
        } else {
            &session.title
        };
        let project_name = session
            .scope
            .project
            .rsplit('/')
            .next()
            .unwrap_or("Project");
        let detail = if session.archived() {
            format!("Archived · {project_name}")
        } else if session.working() {
            format!("Working · {project_name}")
        } else {
            project_name.to_owned()
        };
        let selected = selected.as_deref() == Some(&id);
        rows = rows.child(
            ui::choice(title, &detail, selected, k)
                .key(format!("provider-session-{session_index}"))
                .on_press(move |s: &mut Workspace| select_provider_session(s, session_index)),
        );
    }

    let mut body = col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .gap(10.0)
        .child(
            ui::field(
                &l.commands.sessions_search,
                "Search provider sessions",
                "Search sessions…",
                k,
            )
            .w_full()
            .single_line()
            .edit_menu_changed(
                "provider-session-search",
                |s| Some(&mut live_mut(s).commands.sessions_search),
                |s, _| live_mut(s).commands.sessions_selected = None,
            )
            .on_input_key(sessions_key),
        )
        .child(
            col()
                .key("provider-sessions-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        );
    if matches.is_empty() {
        body = body.child(ui::empty(
            "No matching sessions",
            "Refresh to search this provider’s active and archived sessions.",
            k,
        ));
    }
    if let Some(error) = l.app.notices.get(&scope) {
        body = body.child(ui::caption(error, k).color(k.c.danger.fg));
    }
    let refresh_scope = scope.clone();
    let project_label = project.rsplit('/').next().unwrap_or(&project);
    body.child(ui::divider(k)).child(
        row()
            .w_full()
            .center_y()
            .justify_between()
            .gap(8.0)
            .child(
                ui::caption(
                    format!(
                        "{} · {} sessions · {} · ↑↓ Navigate · ↵ Open",
                        scope.provider.as_str(),
                        matches.len(),
                        project_label
                    ),
                    k,
                )
                .truncate()
                .grow(1.0)
                .min_w(0.0),
            )
            .child(
                Button::new("Refresh")
                    .icon(&ui::REFRESH)
                    .variant(ButtonVariant::Outline)
                    .on_press(move |s: &mut Workspace| {
                        live_mut(s).app.discover_sessions(refresh_scope.clone())
                    })
                    .build(k),
            ),
    )
}

fn select_provider_session(s: &mut Workspace, index: usize) {
    let l = live_mut(s);
    l.app.select(index);
    l.panels.set_overlay(None);
    l.panels.screen = Screen::Conversation;
    l.commands.sessions_search.focused = false;
    l.commands.sessions_selected = None;
    l.current_prompt = None;
}

pub fn sessions_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(owner) = live(s).commands.owner else {
        return false;
    };
    let matches = session_matches(s, owner);
    let selected = live(s).commands.sessions_selected.as_ref();
    let current = matches
        .iter()
        .position(|index| live(s).app.sessions[*index].remote_id.as_ref() == selected);
    let at = match ev.key {
        Key::Down => Some(current.map_or(0, |i| (i + 1).min(matches.len().saturating_sub(1)))),
        Key::Up => Some(current.unwrap_or(0).saturating_sub(1)),
        Key::Enter if !matches.is_empty() => {
            let index = matches[current.unwrap_or(0)];
            select_provider_session(s, index);
            return true;
        }
        _ => return false,
    };
    if let Some(index) = matches.get(at.unwrap_or_default()) {
        let id = live(s).app.sessions[*index].remote_id.clone();
        live_mut(s).commands.sessions_selected = id;
        live_mut(s).commands.sessions_search.focused = true;
        tx.scroll_center_in_instant(
            "provider-sessions-scroll",
            format!("provider-session-{index}"),
        );
    }
    true
}
