use super::{Choice, EditorOwner, command_args};
use crate::live::Live;
use caelum_core::{application::commands::COMMANDS, templates::Target};

pub(super) fn choices(l: &Live, owner: &EditorOwner, query: &str) -> Vec<Choice> {
    let Some(target) = owner.target(l) else {
        return vec![];
    };
    let project = match &target {
        Target::Checkout(p) | Target::Boat(p) => p,
    };
    let make = |name: &str, detail: &str, args: &str, badge| Choice {
        title: format!("/{name}"),
        value: name.into(),
        detail: detail.into(),
        args: args.into(),
        badge,
        project: project.clone(),
    };
    let session = owner.index().map(|i| &l.app.sessions[i]);
    let mut items = Vec::new();
    if let Some(session) = session {
        items.extend(
            COMMANDS
                .iter()
                .filter(|(_, _, c)| c.available(session.scope.provider))
                .map(|(n, d, c)| make(n, d, command_args(*c), None)),
        );
    }
    items.extend(
        l.app
            .templates
            .items(&target)
            .iter()
            .map(|t| make(&t.name, &t.description, &t.argument_hint, Some("Template"))),
    );
    if let Some(s) = session
        && let Some(commands) = l.app.provider_commands.get(&s.scope)
    {
        for c in commands {
            if !items.iter().any(|i| i.value == c.name) {
                items.push(make(
                    &c.name,
                    c.description.as_deref().unwrap_or_default(),
                    "",
                    Some("Custom"),
                ));
            }
        }
    }
    items.retain(|i| {
        format!("{} {}", i.value, i.detail)
            .to_lowercase()
            .contains(query)
    });
    items.sort_by_key(|i| !i.value.eq_ignore_ascii_case(query));
    items
}
