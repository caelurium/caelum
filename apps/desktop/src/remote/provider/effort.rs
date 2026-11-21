use super::{Kind, Menu, Owner, close};
use crate::{
    Workspace,
    live::{Live, live, live_mut},
    select::{Choice, Select, choice_label},
};
use caelum_core::remote::ModelCatalogue;
use rune::El;
use rune_kit::{Kit, Placement};

fn is_open(l: &Live, owner: &Owner) -> bool {
    matches!(&l.remote.menu, Some(Menu {owner: o, kind: Kind::Effort}) if o == owner)
}

fn toggle(s: &mut Workspace, owner: &Owner) {
    let l = live_mut(s);
    if !owner.active(l) {
        return;
    }
    if is_open(l, owner) {
        l.remote.menu = None;
        return;
    }
    l.remote.menu = Some(Menu {
        owner: owner.clone(),
        kind: Kind::Effort,
    });
    if !matches!(l.app.remote.models, ModelCatalogue::Ready(_)) {
        l.app.refresh_vm_models();
    }
}

fn select(s: &mut Workspace, owner: &Owner, value: &str) {
    let l = live_mut(s);
    if is_open(l, owner)
        && owner.active(l)
        && let Some(model) = owner.model.as_deref()
    {
        l.app.set_vm_effort(
            &owner.checkout,
            &owner.thread,
            owner.provider,
            model,
            (!value.is_empty()).then_some(value),
        );
        l.remote.menu = None;
        super::super::conversation::focus(l);
    }
}

pub(super) fn choice(
    s: &Workspace,
    k: &Kit,
    owner: &Owner,
    current: Option<&str>,
    layers: &mut Vec<El>,
) -> Option<El> {
    // The public catalogue's harness default isn't the user's dashboard model.
    let id = owner.model.as_deref()?;
    let l = live(s);
    let catalogue = &l.app.remote.models;
    let model = catalogue.find(owner.provider, id);
    let levels = model.map_or(&[][..], |m| m.efforts.as_slice());
    let open = is_open(l, owner);
    if !open && matches!(catalogue, ModelCatalogue::Ready(_)) && levels.is_empty() {
        return None;
    }
    let detail = match catalogue {
        ModelCatalogue::Loading(_) | ModelCatalogue::Unloaded => "Loading effort levels from Boat…",
        ModelCatalogue::Failed(_) => "Couldn’t load effort levels. Reopen to retry.",
        ModelCatalogue::Ready(_) if levels.is_empty() => {
            "Boat lists no effort levels for this model."
        }
        ModelCatalogue::Ready(_) => "Depth of reasoning for the next message.",
    };
    let mut default = Choice::new("", "Boat default");
    if let Some(level) = model
        .and_then(|m| m.default_effort.as_ref())
        .and_then(|id| levels.iter().find(|e| e.id == *id))
    {
        default = default.detail(format!("Model’s listed default: {}", choice_label(level)));
    }
    let options = std::iter::once(default)
        .chain(levels.iter().map(|e| Choice::new(&e.id, choice_label(e))))
        .collect();
    let display = levels
        .iter()
        .find(|e| Some(e.id.as_str()) == current)
        .map(choice_label)
        .or(current)
        .unwrap_or("Effort");
    let target = owner.clone();
    let choose = owner.clone();
    Some(
        Select::choices("vm-effort", options)
            .label("Reasoning effort")
            .current(Some(current.unwrap_or("")))
            .display(display)
            .width(260.)
            .menu_description(detail)
            .placement(Placement::TopStart)
            .open(open)
            .disabled(!owner.active(l))
            .on_toggle(move |s| toggle(s, &target))
            .on_close(close)
            .on_choose(move |s, value, _| select(s, &choose, value))
            .build(k, layers)
            .max_w(108.),
    )
}
