//! Searchable, branded model catalogue. The picker owns its target until it closes.
mod brand;
mod catalog;
#[cfg(test)]
mod tests;
mod view;

pub use view::overlay;

use crate::{
    Workspace,
    live::{live, live_mut},
};
use caelum_core::application::{Provider, Scope};
use catalog::{Model, matches};
use rune::{Key, KeyEvent, TextEdit, Tx};

#[derive(Clone)]
enum Owner {
    Home(Scope),
    Session(usize, Scope),
    Defaults(Scope),
}

impl Owner {
    fn scope(&self) -> &Scope {
        match self {
            Self::Home(scope) | Self::Session(_, scope) | Self::Defaults(scope) => scope,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Favorites,
    Connection(String),
}

pub struct Picker {
    pub query: TextEdit,
    owner: Option<Owner>,
    selected: Option<String>,
    filter: Filter,
    pub reveal: Option<usize>,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            query: TextEdit::new(""),
            owner: None,
            selected: None,
            filter: Filter::All,
            reveal: None,
        }
    }
}

impl Picker {
    pub fn is_open(&self) -> bool {
        self.owner.is_some()
    }
}

pub fn open(s: &mut Workspace, settings: Option<Provider>) {
    if live(s).models.is_open() {
        close(s);
        return;
    }
    let l = live_mut(s);
    let owner = if let Some(provider) = settings {
        let Some(project) = l.app.active_project.clone() else {
            return;
        };
        Owner::Defaults(Scope { project, provider })
    } else if l.panels.screen == crate::panels::Screen::Home {
        let Some(scope) = l.app.home_scope() else {
            return;
        };
        Owner::Home(scope)
    } else {
        let Some(index) = l.app.selected else { return };
        Owner::Session(index, l.app.sessions[index].scope.clone())
    };
    let choices_scope = owner.scope().clone();
    let needs_choices = l
        .app
        .choices
        .get(&choices_scope)
        .is_none_or(|choices| choices.models.is_empty());
    l.models.selected = match &owner {
        Owner::Home(_) => l.app.home_options().model,
        Owner::Session(i, _) => l.app.sessions[*i].options.model.clone(),
        Owner::Defaults(scope) => l.app.provider_options(scope).and_then(|o| o.model.clone()),
    };
    l.models.owner = Some(owner);
    l.models.filter = Filter::All;
    l.models.query.clear();
    l.models.query.focused = true;
    if needs_choices {
        l.app.refresh_choices_for(choices_scope);
    }
    l.panels.settings_menu = None;
    for draft in &mut l.drafts {
        draft.focused = false;
    }
    l.commands.side_editor.focused = false;
    l.home.editor.focused = false;
    let at = selected_index(s, &matches(s));
    live_mut(s).models.reveal = Some(at);
}

pub fn close(s: &mut Workspace) {
    let l = live_mut(s);
    l.models.query.focused = false;
    if matches!(
        l.models.owner.take(),
        Some(Owner::Home(_) | Owner::Session(..))
    ) {
        l.focus();
    }
}

fn selected_index(s: &Workspace, items: &[Model]) -> usize {
    items
        .iter()
        .position(|m| Some(&m.id) == live(s).models.selected.as_ref())
        .unwrap_or(0)
}

fn filter(s: &mut Workspace, filter: Filter, tx: &mut Tx) {
    let picker = &mut live_mut(s).models;
    picker.filter = filter;
    picker.selected = None;
    picker.reveal = None;
    picker.query.focused = true;
    tx.scroll_to_y("model-scroll", 0.0);
}

fn favorite(s: &mut Workspace, provider: Provider, id: &str) {
    let l = live_mut(s);
    let Some(scope) = l
        .models
        .owner
        .as_ref()
        .map(Owner::scope)
        .filter(|s| s.provider == provider)
    else {
        return;
    };
    l.app.toggle_favorite_model(scope, id);
    l.models.selected = Some(id.to_owned());
    l.models.query.focused = true;
    let at = selected_index(s, &matches(s));
    live_mut(s).models.reveal = Some(at);
}

fn choose(s: &mut Workspace, id: &str) {
    let l = live_mut(s);
    let Some(owner) = l.models.owner.clone() else {
        return;
    };
    let model = if id.is_empty() && matches!(owner, Owner::Session(..)) {
        l.app
            .choices
            .get(owner.scope())
            .and_then(|c| c.default_model.clone())
    } else {
        (!id.is_empty()).then(|| id.to_owned())
    };
    let effort = l
        .app
        .choices
        .get(owner.scope())
        .and_then(|c| c.models.iter().find(|m| Some(&m.id) == model.as_ref()))
        .and_then(|m| m.default_effort.clone());
    match owner {
        Owner::Home(scope) => {
            if l.app.home_scope().as_ref() != Some(&scope) {
                close(s);
                return;
            }
            let mut options = l.app.home_options();
            options.model = model;
            options.effort = effort;
            l.app.home.draft.provider = Some(scope.provider);
            l.app.home.draft.options = options;
        }
        Owner::Defaults(scope) => {
            l.app.edit_provider_options(&scope, |defaults| {
                defaults.model = model;
                defaults.effort = effort;
            });
        }
        Owner::Session(index, scope) => {
            let session = &mut l.app.sessions[index];
            if session.scope != scope
                || session.archived()
                || session.forgotten()
                || session.archiving()
            {
                close(s);
                return;
            }
            session.options.model = model;
            session.options.effort = effort;
            l.app.options_changed(index);
        }
    }
    close(s);
}

pub fn key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let items = matches(s);
    let current = selected_index(s, &items);
    match ev.key {
        Key::Down | Key::Up => {
            let next = if ev.key == Key::Down {
                (current + 1).min(items.len().saturating_sub(1))
            } else {
                current.saturating_sub(1)
            };
            live_mut(s).models.selected = items.get(next).map(|m| m.id.clone());
            live_mut(s).models.query.focused = true;
            tx.scroll_center_in_instant("model-list", format!("option{next}"));
            true
        }
        Key::Enter => {
            if let Some(m) = items.get(current).filter(|m| m.available) {
                choose(s, &m.id);
            }
            true
        }
        Key::Escape => {
            close(s);
            true
        }
        _ => ['n', 'p', 'o', 'l', 'k', ',', 'i']
            .iter()
            .any(|c| ev.is_command(*c)),
    }
}
