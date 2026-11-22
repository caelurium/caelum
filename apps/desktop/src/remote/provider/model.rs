use super::{Kind, Menu, Owner, close};
use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    select::Select,
    ui,
};
use caelum_core::remote::ModelCatalogue;
use caelum_protocol::ProviderModelChoice;
use rune::{El, ScrollSpec, TextEdit, col};
use rune_kit::{Kit, Placement, ScrollBar};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

const TRIGGER: &str = "vm-model-trigger";
const ROW_HEIGHT: f32 = 46.;

pub(super) fn choice(s: &Workspace, k: &Kit, owner: &Owner, layers: &mut Vec<El>) -> El {
    let l = live(s);
    let current = owner.model.as_deref();
    let label = current.map_or("Boat default", |id| {
        l.app
            .remote
            .models
            .find(owner.provider, id)
            .map_or(id, |m| m.name.as_str())
    });
    let query = match &l.remote.menu {
        Some(Menu {
            owner: o,
            kind: Kind::Model(q),
        }) if o == owner => Some(q.as_ref()),
        _ => None,
    };
    let open = query.is_some();
    let enabled = owner.active(l);
    if enabled && let Some(query) = query {
        let body = menu(s, k, owner, current, query);
        ui::popup(
            "vm-model-menu",
            TRIGGER,
            body,
            Placement::TopStart,
            close,
            k,
            layers,
        );
    }
    let target = owner.clone();
    let width =
        rune::memo::window_layout("main-pane").map_or(170., |r| (r[2] - 160.).clamp(54., 170.));
    Select::new::<&str>("vm-model", &[])
        .label("VM model")
        .current(Some(label))
        .open(open)
        .disabled(!enabled)
        .on_toggle(move |s: &mut Workspace| {
            let l = live_mut(s);
            if open {
                l.remote.menu = None;
                return;
            }
            if !target.active(l) {
                return;
            }
            let mut query = TextEdit::new("");
            query.focused = true;
            l.remote.menu = Some(Menu {
                owner: target.clone(),
                kind: Kind::Model(Box::new(query)),
            });
            if !matches!(l.app.remote.models, ModelCatalogue::Ready(_)) {
                l.app.refresh_vm_models();
            }
        })
        .trigger(k)
        .max_w(width)
}

fn choose(s: &mut Workspace, owner: &Owner, model: Option<&str>) {
    let l = live_mut(s);
    if matches!(&l.remote.menu, Some(Menu {owner:o,kind:Kind::Model(_)}) if o == owner)
        && owner.active(l)
    {
        l.app
            .set_vm_model(&owner.checkout, &owner.thread, owner.provider, model);
        l.remote.menu = None;
        super::super::conversation::focus(l);
    }
}

fn matches_query(model: &ProviderModelChoice, needle: &str) -> bool {
    needle.is_empty()
        || model.name.to_lowercase().contains(needle)
        || model.id.to_lowercase().contains(needle)
}

fn submit_search(s: &mut Workspace, target: &Owner) {
    let l = live(s);
    let Some(Menu {
        owner,
        kind: Kind::Model(query),
    }) = &l.remote.menu
    else {
        return;
    };
    let ModelCatalogue::Ready(lists) = &l.app.remote.models else {
        return;
    };
    let needle = query.text.trim().to_lowercase();
    if needle.is_empty() || owner != target {
        return;
    }
    let Some(list) = lists.get(&target.provider) else {
        return;
    };
    let mut found = list.iter().filter(|m| matches_query(m, &needle));
    if let (Some(m), None) = (found.next(), found.next()) {
        let id = m.id.clone();
        choose(s, target, Some(&id));
    }
}

fn menu(s: &Workspace, k: &Kit, owner: &Owner, current: Option<&str>, query: &TextEdit) -> El {
    let catalogue = &live(s).app.remote.models;
    let default = owner.clone();
    let search = owner.clone();
    let submit = owner.clone();
    let window = rune::memo::window_layout("shell").unwrap_or([0., 0., 800., 620.]);
    let refresh = crate::quiet_button(&ui::REFRESH, "Refresh Boat models")
        .disabled(matches!(catalogue, ModelCatalogue::Loading(_)))
        .on_press(|s: &mut Workspace| live_mut(s).app.refresh_vm_models())
        .build(k)
        .key("vm-model-refresh");
    col()
        .w((window[2] - 16.).clamp(1., 320.))
        .gap(6.)
        .child(ui::popup_header(
            format!("{} models on Boat", owner.provider.name()),
            None,
            Some(refresh),
            k,
        ))
        .child(
            ui::field(query, "Find a Boat model", "Search models…", k)
                .edit_menu_changed(
                    "vm-model-search",
                    move |s| match &mut live_mut(s).remote.menu {
                        Some(Menu {
                            owner,
                            kind: Kind::Model(q),
                        }) if owner == &search => Some(q.as_mut()),
                        _ => None,
                    },
                    |_, tx| tx.scroll_to_y("vm-model-scroll", 0.),
                )
                .on_submit(move |s: &mut Workspace, _| submit_search(s, &submit))
                .help("Search by name or ID · Enter chooses a single match"),
        )
        .child(
            ui::choice(
                "Boat default",
                "Use the model set in Agents & Proxying",
                current.is_none(),
                k,
            )
            .on_press(move |s: &mut Workspace| choose(s, &default, None))
            .key("vm-model-default"),
        )
        .child(results(catalogue, k, owner, current, &query.text))
}

fn results(
    catalogue: &ModelCatalogue,
    k: &Kit,
    owner: &Owner,
    current: Option<&str>,
    query: &str,
) -> El {
    let lists = match catalogue {
        ModelCatalogue::Ready(lists) => lists,
        ModelCatalogue::Failed(e) => {
            return ui::caption(format!("Couldn’t load models: {e}"), k)
                .wrap()
                .px(6.)
                .role("status");
        }
        _ => return ui::caption("Loading Boat models…", k).px(6.).role("status"),
    };
    let models = lists.get(&owner.provider).cloned().unwrap_or_default();
    let needle = query.trim().to_lowercase();
    let mut version = DefaultHasher::new();
    let matches: Rc<Vec<_>> = Rc::new(
        models
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                if matches_query(m, &needle) {
                    m.id.hash(&mut version);
                    Some(i)
                } else {
                    None
                }
            })
            .collect(),
    );
    if matches.is_empty() {
        return ui::caption(
            if models.is_empty() {
                "No models listed for this provider."
            } else {
                "No matching models."
            },
            k,
        )
        .px(6.);
    }
    let count = matches.len();
    let keys = (models.clone(), matches.clone());
    let kit = k.clone();
    let owner = owner.clone();
    let current = current.map(str::to_owned);
    let rows = col()
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count,
            ROW_HEIGHT,
            move |_: &Workspace, i| format!("vm-model-item-{}", keys.0[keys.1[i]].id).into(),
            move |_: &Workspace, i| {
                let m = &models[matches[i]];
                let target = owner.clone();
                let id = m.id.clone();
                ui::choice(&m.name, &m.id, current.as_ref() == Some(&id), &kit)
                    .help(&m.id)
                    .on_press(move |s: &mut Workspace| choose(s, &target, Some(&id)))
            },
        )
        .virtual_rows_version(version.finish());
    let window = rune::memo::window_layout("shell").unwrap_or([0., 0., 800., 620.]);
    let space = rune::memo::window_layout(TRIGGER).map_or(window[3], |r| {
        (r[1] - window[1]).max(window[1] + window[3] - r[1] - r[3])
    });
    // Leave room for the search field and the always-available Boat default row.
    let list_height = (count.min(5) as f32 * ROW_HEIGHT).min((space - 158.).max(ROW_HEIGHT));
    col().w_full().gap(4.).child(ui::divider(k)).child(
        col()
            .w_full()
            .h(list_height)
            .min_h(0.)
            .key("vm-model-scroll")
            .scroll(ScrollSpec {
                stick_bottom: false,
            })
            .child(rows)
            .scroll_bar(k),
    )
}
