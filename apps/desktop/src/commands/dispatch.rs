use super::{ask, refresh_stats};
use crate::{
    Workspace,
    live::{live, live_mut},
    panels::{self, Screen},
};
use caelum_core::application::{Task, commands::LocalCommand};
use rune::{TextEdit, Tx};

pub fn dispatch(s: &mut Workspace, tx: &mut Tx) -> bool {
    let Some(index) = live(s).app.selected else {
        return false;
    };
    let input = live(s).drafts[index].text.clone();
    let command = match LocalCommand::parse(&input, live(s).app.sessions[index].scope.provider) {
        Ok(Some(command)) => command,
        Ok(None) => return false,
        Err(error) => {
            live_mut(s).app.sessions[index].error = Some(error);
            return true;
        }
    };
    let (command, args) = command;
    let l = live_mut(s);
    if command == LocalCommand::SideQuestion && l.app.sessions[index].remote_id.is_none() {
        l.app.sessions[index].error =
            Some("Open an OpenCode session before asking a side question.".into());
        return true;
    }
    l.app.sessions[index].error = None;
    l.drafts[index].clear();
    l.app.sessions[index].draft.clear();
    l.commands.owner = Some(index);
    l.commands.search.clear();
    match command {
        LocalCommand::New => l.new_session(),
        LocalCommand::Sessions => {
            let scope = l.app.sessions[index].scope.clone();
            l.commands.sessions_search.clear();
            l.commands.sessions_selected = None;
            l.commands.sessions_search.focused = true;
            l.panels.overlay = Some(Screen::Sessions);
            l.app.discover_sessions(scope);
        }
        LocalCommand::Models => crate::models::open(s, None),
        LocalCommand::Variants => open_variants(s, index),
        LocalCommand::Effort => {
            if args.is_empty() {
                open_variants(s, index);
            } else {
                set_effort(s, index, args);
            }
        }
        LocalCommand::Files => panels::show(s, Screen::Files),
        LocalCommand::Changes => panels::show(s, Screen::Changes),
        LocalCommand::Settings => panels::show(s, Screen::Settings),
        LocalCommand::Timeline => {
            l.commands.timeline_selected = l.current_prompt.clone();
            l.commands.timeline_current = l.current_prompt.clone();
            l.panels.overlay = Some(Screen::Timeline);
            l.commands.search.focused = true;
        }
        LocalCommand::Usage => {
            l.panels.overlay = Some(Screen::Usage);
            let project = l.app.sessions[index].scope.project.clone();
            l.app.task(Task::Usage { project });
        }
        LocalCommand::Stats => {
            l.commands.stats_anchor = caelum_core::application::presentation::now();
            l.panels.overlay = Some(Screen::Stats);
            refresh_stats(s);
        }
        LocalCommand::SideQuestion => {
            l.app.dismiss_side_question(index);
            l.commands.side_owner = Some(index);
            l.commands.side_editor = TextEdit::new(args);
            l.commands.side_editor.focused = true;
            let previous = l
                .commands
                .previous_drafts
                .get(index)
                .cloned()
                .unwrap_or_default();
            l.drafts[index].replace_text(previous);
            l.drafts[index].focused = false;
            if !args.is_empty() {
                ask(s, tx);
            }
        }
    }
    true
}

fn open_variants(s: &mut Workspace, index: usize) {
    let session = &live(s).app.sessions[index];
    let choices = live(s).app.choices.get(&session.scope);
    let model_id = session
        .options
        .model
        .as_ref()
        .or_else(|| choices.and_then(|choices| choices.default_model.as_ref()));
    let has_variants = choices
        .and_then(|choices| {
            choices
                .models
                .iter()
                .find(|model| Some(&model.id) == model_id)
        })
        .is_some_and(|model| !model.efforts.is_empty());
    if has_variants {
        crate::live::open_effort(s, index);
    } else {
        live_mut(s).app.sessions[index].error =
            Some("No model effort variants are available for the selected model.".into());
    }
}

fn set_effort(s: &mut Workspace, index: usize, value: &str) {
    let scope = live(s).app.sessions[index].scope.clone();
    let choices = live(s).app.choices.get(&scope);
    let model_id = live(s).app.sessions[index]
        .options
        .model
        .clone()
        .or_else(|| choices.and_then(|choices| choices.default_model.clone()));
    let reset = matches!(
        value.to_ascii_lowercase().as_str(),
        "default" | "model default"
    );

    let effort = if reset {
        Some(None)
    } else {
        choices
            .and_then(|choices| {
                choices
                    .models
                    .iter()
                    .find(|model| Some(&model.id) == model_id.as_ref())
            })
            .and_then(|model| {
                model
                    .efforts
                    .iter()
                    .find(|effort| {
                        effort.id.eq_ignore_ascii_case(value)
                            || effort.name.eq_ignore_ascii_case(value)
                    })
                    .map(|effort| Some(effort.id.clone()))
            })
    };

    if let Some(effort) = effort {
        live_mut(s).app.set_effort(index, effort);
        return;
    }

    let available = choices
        .and_then(|choices| {
            choices
                .models
                .iter()
                .find(|model| Some(&model.id) == model_id.as_ref())
        })
        .map(|model| {
            model
                .efforts
                .iter()
                .map(|effort| effort.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| "none".into());
    live_mut(s).app.sessions[index].error = Some(format!(
        "Unknown effort ‘{value}’. Available efforts: {available}. Use /effort default to reset."
    ));
}
