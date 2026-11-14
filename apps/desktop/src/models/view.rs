use super::{
    Filter, Owner, brand,
    catalog::{self, Group, Model},
    choose, close, favorite, filter, key, selected_index,
};
use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    ui,
};
use rune::{Color, El, Icon, ScrollSpec, TextStyle, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit, ScrollBar};
use std::{collections::BTreeMap, rc::Rc};

const STAR: Icon = Icon {
    name: "model-favorite",
    view: 24.0,
    body: r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>"#,
};
const STAR_FILLED: Icon = Icon {
    name: "model-favorite-filled",
    view: 24.0,
    body: r#"<path d="m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9Z" fill="currentColor"/>"#,
};

fn chip(label: &str, count: usize, selected: bool, glyph: Option<&'static Icon>, k: &Kit) -> El {
    k.interactive(
        row()
            .h(30.0)
            .px(9.0)
            .gap(6.0)
            .center_y()
            .radius(7.0)
            .fill(if selected {
                k.c.pressed
            } else {
                Color::TRANSPARENT
            })
            .border(
                1.0,
                if selected {
                    k.c.border_strong
                } else {
                    k.c.border_base
                },
            )
            .children(glyph.map(|glyph| icon(glyph, 16.0).color(k.c.text_base).a11y_hidden()))
            .child(text(label).font(k.ty.small).color(if selected {
                k.c.text_base
            } else {
                k.c.text_muted
            }))
            .child(
                text(count.to_string())
                    .font(k.ty.small)
                    .color(k.c.text_muted),
            ),
    )
    .role("button")
    .label(label)
    .a11y_value(if selected { "Selected" } else { "" })
    .focusable(true)
    .focus_ring(1.5, 0.0, k.c.border_focus)
}

fn filters(s: &Workspace, items: &[Model], k: &Kit) -> El {
    let active = &live(s).models.filter;
    let count = items
        .iter()
        .filter(|m| !m.id.is_empty() && m.available)
        .count();
    let favorites = items.iter().filter(|m| m.favorite).count();
    let mut connections = BTreeMap::<&str, (&str, usize)>::new();
    for m in items.iter().filter(|m| !m.id.is_empty() && m.available) {
        let entry = connections
            .entry(&m.connection_id)
            .or_insert((&m.connection, 0));
        entry.1 += 1;
    }
    let mut rail = row()
        .gap(6.0)
        .center_y()
        .child(
            chip("All", count, *active == Filter::All, None, k)
                .key("model-filter-all")
                .on_press_tx(|s: &mut Workspace, tx| filter(s, Filter::All, tx)),
        )
        .child(
            chip(
                "Favourites",
                favorites,
                *active == Filter::Favorites,
                Some(&STAR),
                k,
            )
            .key("model-filter-favorites")
            .on_press_tx(|s: &mut Workspace, tx| filter(s, Filter::Favorites, tx)),
        );
    for (id, (name, count)) in connections {
        let selected = *active == Filter::Connection(id.into());
        let glyph = brand::Brand::provider(id).map(brand::Brand::glyph);
        let target = id.to_owned();
        rail = rail.child(
            chip(name, count, selected, glyph, k)
                .key(format!("model-filter-connection-{id}"))
                .help(format!("Models available through {name}"))
                .on_press_tx(move |s: &mut Workspace, tx| {
                    filter(s, Filter::Connection(target.clone()), tx)
                }),
        );
    }
    col()
        .key("model-filters")
        .w_full()
        .h(34.0)
        .scroll_x()
        .child(rail.self_start())
}

fn model_row(
    m: &Model,
    i: usize,
    current: &str,
    selected: usize,
    provider: caelum_core::application::Provider,
    k: &Kit,
) -> El {
    let id = m.id.clone();
    let title = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(
            text(&m.name)
                .font(TextStyle::new(13.0, 550.0))
                .color(k.c.text_base)
                .truncate(),
        )
        .child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    text(&m.connection)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate()
                        .min_w(0.0),
                )
                .children((!m.context.is_empty()).then(|| {
                    text(format!("· {}", m.context))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                })),
        );
    let mark = if m.id.is_empty() {
        icon(&ui::CHOICE, 20.0).color(k.c.text_muted).a11y_hidden()
    } else {
        brand::logo(m.brand, &m.connection, 28.0, k)
    };
    let choice = k
        .interactive(
            row()
                .grow(1.0)
                .min_w(0.0)
                .h(58.0)
                .px(9.0)
                .gap(10.0)
                .center_y()
                .radius(8.0)
                .fill(if i == selected {
                    k.c.hover
                } else {
                    Color::TRANSPARENT
                })
                .child(
                    col()
                        .size(34.0, 34.0)
                        .center()
                        .radius(8.0)
                        .fill(k.c.layer_02)
                        .child(mark),
                )
                .child(title)
                .child(
                    icon(k.glyph.check_small, 14.0)
                        .color(k.c.text_accent)
                        .opacity(if m.id == current { 1.0 } else { 0.0 }),
                ),
        )
        .role("button")
        .label(&m.name)
        .description(format!("{} · {} · {}", m.company, m.connection, m.context))
        .help(if m.id.is_empty() {
            m.connection.clone()
        } else {
            format!("{}\n{}", m.connection, m.id)
        })
        .disabled(!m.available)
        .focusable(m.available)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .opacity(if m.available { 1.0 } else { 0.55 })
        .on_press(move |s: &mut Workspace| choose(s, &id));
    let mut entry = row().w_full().h(62.0).gap(2.0).center_y().child(choice);
    if !m.id.is_empty() {
        let id = m.id.clone();
        entry = entry.child(
            crate::quiet_button(
                if m.favorite { &STAR_FILLED } else { &STAR },
                if m.favorite {
                    "Remove favourite"
                } else {
                    "Favourite model"
                },
            )
            .on_press(move |s: &mut Workspace| favorite(s, provider, &id))
            .build(k)
            .key(format!("model-favorite-{i}"))
            .help(if m.favorite {
                "Remove favourite"
            } else {
                "Pin to favourites"
            }),
        );
    }
    entry
}

fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let owner = l.models.owner.as_ref().unwrap();
    let scope = owner.scope();
    let provider = scope.provider;
    let current = match owner {
        Owner::Session(i, _) => l.app.sessions[*i].options.model.as_deref(),
        Owner::Defaults(scope) => l
            .app
            .preferences
            .providers
            .get(scope.provider.as_str())
            .and_then(|o| o.model.as_deref()),
    }
    .unwrap_or_default()
    .to_owned();
    let all = catalog::all(s);
    let items = Rc::new(catalog::matches(s));
    let selected = selected_index(s, &items);
    let count = items.len();
    let model_count = items.iter().filter(|m| !m.id.is_empty()).count();
    let kit = k.clone();
    let rows = col()
        .key("model-list")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count,
            62.0,
            |_: &Workspace, i| format!("option{i}").into(),
            move |_: &Workspace, i| {
                let m = &items[i];
                let entry = model_row(m, i, &current, selected, provider, &kit);
                if m.group != Group::Default && (i == 0 || items[i - 1].group != m.group) {
                    let count = items.iter().filter(|other| other.group == m.group).count();
                    col()
                        .w_full()
                        .child(
                            row()
                                .h(30.0)
                                .px(10.0)
                                .gap(8.0)
                                .center_y()
                                .key(format!("model-group-{}", m.group.label()))
                                .child(
                                    text(m.group.label())
                                        .font(kit.ty.small)
                                        .color(kit.c.text_base),
                                )
                                .child(
                                    text(count.to_string())
                                        .font(kit.ty.small)
                                        .color(kit.c.text_muted),
                                )
                                .child(ui::divider(&kit).grow(1.0).w(0.0)),
                        )
                        .child(entry)
                } else {
                    entry
                }
            },
        );
    let mut body = col()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(10.0)
        .child(
            ui::field(
                &l.models.query,
                "Search models",
                "Search models or companies…",
                k,
            )
            .edit_menu_changed(
                "model-search",
                |s| Some(&mut live_mut(s).models.query),
                |s, tx| {
                    live_mut(s).models.selected = None;
                    live_mut(s).models.reveal = None;
                    tx.scroll_to_y("model-scroll", 0.0);
                },
            )
            .on_input_key(key),
        )
        .child(filters(s, &all, k));
    if let Some(error) = l.app.notices.get(scope) {
        body = body.child(ui::notice(
            &format!("{} unavailable", ui::provider_name(provider)),
            error,
            k,
        ));
    }
    let mut body = body
        .child(if count == 0 {
            ui::empty(
                if l.models.filter == Filter::Favorites {
                    "No favourite models"
                } else {
                    "No matching models"
                },
                if l.models.filter == Filter::Favorites && l.models.query.text.trim().is_empty() {
                    "Star a model to keep it at the top of your list."
                } else {
                    "Try another search or connection."
                },
                k,
            )
            .grow(1.0)
        } else {
            col()
                .key("model-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows)
                .scroll_bar(k)
        })
        .child(ui::divider(k))
        .child(
            row()
                .w_full()
                .justify_between()
                .center_y()
                .child(
                    ui::caption(
                        format!(
                            "{model_count} {}",
                            if model_count == 1 { "model" } else { "models" }
                        ),
                        k,
                    )
                    .key("model-result-count"),
                )
                .child(ui::caption("↑↓ Navigate   ↵ Choose", k)),
        );
    if let Owner::Session(index, scope) = owner {
        let index = *index;
        let scope = scope.clone();
        body = body.child(
            ui::Button::new("Use current settings for new sessions")
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    if l.app.sessions[index].scope == scope {
                        l.app.preferences.providers.insert(
                            scope.provider.as_str().into(),
                            l.app.sessions[index].options.clone(),
                        );
                        l.app.preferences.default_provider = scope.provider;
                    }
                })
                .build(k)
                .key("save-provider-defaults"),
        );
    }
    body
}

pub fn overlay(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(owner) = live(s).models.owner.as_ref() else {
        return;
    };
    let defaults = matches!(owner, Owner::Defaults(_));
    let anchor = if defaults {
        "settings-model-trigger"
    } else {
        "model-trigger"
    };
    let window = rune::memo::window_layout("shell");
    let mut width = window.map_or(520.0, |r| (r[2] - 32.0).min(520.0));
    let height = window.map_or(520.0, |r| (r[3] - 16.0).min(520.0));
    let mut placement = if defaults {
        rune_kit::Placement::BottomStart
    } else {
        rune_kit::Placement::TopStart
    };
    // A settings field sits mid-window. In compact windows, use the room
    // beside it rather than squeezing the catalogue into a single model row.
    if defaults
        && let Some(window) = window
        && let Some(trigger) = rune::memo::window_layout(anchor)
        && trigger[1].max(window[3] - trigger[1] - trigger[3]) < 400.0
        && trigger[0] >= 416.0
    {
        width = width.min(trigger[0] - 16.0);
        placement = rune_kit::Placement::Left;
    }
    layers.push(
        rune::el()
            .key("models-scrim")
            .role("none")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .on_press(close),
    );
    let content = col()
        .key("model-popover")
        .role("dialog")
        .label("Choose model")
        .modal(true, close)
        .focus_return(anchor)
        .w(width)
        .h(height)
        .min_h(0.0)
        .padding(12.0)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(
                    col()
                        .gap(3.0)
                        .child(
                            text("Choose a model")
                                .font(TextStyle::new(15.0, 600.0))
                                .color(k.c.text_base),
                        )
                        .child(ui::caption(
                            format!(
                                "{} · Models grouped by company",
                                ui::provider_name(owner.scope().provider)
                            ),
                            k,
                        )),
                )
                .child(
                    crate::quiet_button(&rune_kit::glyphs::CLOSE, "Close model picker")
                        .on_press(close)
                        .build(k),
                ),
        )
        .child(view(s, k));
    layers.push(rune_kit::popover::anchored(
        k,
        rune_kit::popover::raised(k, content, 12.0),
        anchor,
        placement,
        8.0,
    ));
}
