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

const ROW_HEIGHT: f32 = 36.0;
const GROUP_HEIGHT: f32 = 22.0;
const MAX_WIDTH: f32 = 400.0;
const MAX_HEIGHT: f32 = 360.0;

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

fn chip(label: &str, selected: bool, glyph: Option<&'static Icon>, k: &Kit) -> El {
    k.interactive(
        row()
            .h(26.0)
            .px(7.0)
            .gap(5.0)
            .center_y()
            .radius(5.0)
            .fill(if selected {
                k.c.hover
            } else {
                Color::TRANSPARENT
            })
            .children(glyph.map(|glyph| icon(glyph, 14.0).color(k.c.text_muted).a11y_hidden()))
            .child(text(label).font(k.ty.small).color(if selected {
                k.c.text_base
            } else {
                k.c.text_muted
            })),
    )
    .role("button")
    .label(label)
    .a11y_value(if selected { "Selected" } else { "" })
    .focusable(true)
    .focus_ring(1.5, 0.0, k.c.border_focus)
}

fn filters(s: &Workspace, items: &[Model], k: &Kit) -> El {
    let active = &live(s).models.filter;
    let mut connections = BTreeMap::<&str, &str>::new();
    for m in items.iter().filter(|m| !m.id.is_empty() && m.available) {
        connections.insert(&m.connection_id, &m.connection);
    }
    let mut rail = row()
        .gap(2.0)
        .center_y()
        .child(
            chip("All", *active == Filter::All, None, k)
                .key("model-filter-all")
                .on_press_tx(|s: &mut Workspace, tx| filter(s, Filter::All, tx)),
        )
        .child(
            chip("Favourites", *active == Filter::Favorites, Some(&STAR), k)
                .key("model-filter-favorites")
                .on_press_tx(|s: &mut Workspace, tx| filter(s, Filter::Favorites, tx)),
        );
    for (id, name) in connections.iter().filter(|_| connections.len() > 1) {
        let selected = *active == Filter::Connection((*id).into());
        let glyph = brand::Brand::provider(id).map(brand::Brand::glyph);
        let target = (*id).to_owned();
        rail = rail.child(
            chip(name, selected, glyph, k)
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
        .h(26.0)
        .scroll_x()
        .child(rail.self_start())
}

fn model_row(
    s: &Workspace,
    m: &Model,
    i: usize,
    current: &str,
    selected: usize,
    provider: caelum_core::application::Provider,
    k: &Kit,
) -> El {
    let id = m.id.clone();
    let mark = if m.id.is_empty() {
        icon(&ui::CHOICE, 20.0).color(k.c.text_muted).a11y_hidden()
    } else {
        brand::logo(m.brand, &m.connection, 20.0, k)
    };
    let choice = k
        .interactive(
            row()
                .grow(1.0)
                .min_w(0.0)
                .h(ROW_HEIGHT - 2.0)
                .px(6.0)
                .gap(7.0)
                .center_y()
                .radius(5.0)
                .fill(if i == selected {
                    k.c.hover
                } else {
                    Color::TRANSPARENT
                })
                .child(mark)
                .child(
                    text(&m.name)
                        .font(TextStyle::new(13.0, 500.0))
                        .color(k.c.text_base)
                        .truncate()
                        .min_w(0.0)
                        .grow(1.0),
                )
                .children(
                    (!m.id.is_empty() && !matches!(live(s).models.filter, Filter::Connection(_)))
                        .then(|| {
                            text(&m.connection)
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .truncate()
                                .max_w(100.0)
                                .min_w(0.0)
                        }),
                )
                .child(
                    icon(k.glyph.check_small, 12.0)
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
            format!(
                "{}\n{}{}",
                m.connection,
                m.id,
                if m.context.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", m.context)
                }
            )
        })
        .disabled(!m.available)
        .focusable(m.available)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .opacity(if m.available { 1.0 } else { 0.55 })
        .on_press(move |s: &mut Workspace| choose(s, &id));
    let mut entry = row()
        .w_full()
        .h(ROW_HEIGHT)
        .gap(2.0)
        .center_y()
        .hover_group()
        .child(choice);
    if !m.id.is_empty() {
        let id = m.id.clone();
        let favorite_key = format!("model-favorite-{i}");
        let visible =
            m.favorite || i == selected || live(s).focus_key.as_deref() == Some(&favorite_key);
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
            .key(favorite_key)
            .group_opacity(if visible { 1.0 } else { 0.0 }, 1.0)
            .help(if m.favorite {
                "Remove favourite"
            } else {
                "Pin to favourites"
            }),
        );
    }
    entry
}

fn view(s: &Workspace, k: &Kit) -> (El, f32) {
    let l = live(s);
    let owner = l.models.owner.as_ref().unwrap();
    let scope = owner.scope();
    let provider = scope.provider;
    let home_options = l.app.home_options();
    let current = match owner {
        Owner::Home(_) => home_options.model.as_deref(),
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
    let groups = items
        .iter()
        .enumerate()
        .filter(|(i, m)| m.group != Group::Default && (*i == 0 || items[*i - 1].group != m.group))
        .count();
    // Search, filters and one quiet footer; short result sets shrink naturally.
    let height = if l.app.notices.contains_key(scope) {
        MAX_HEIGHT
    } else {
        (125.0 + (count as f32 * ROW_HEIGHT + groups as f32 * GROUP_HEIGHT).max(80.0))
            .min(MAX_HEIGHT)
    };
    let kit = k.clone();
    let rows = col()
        .key("model-list")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count,
            ROW_HEIGHT,
            |_: &Workspace, i| format!("option{i}").into(),
            move |s: &Workspace, i| {
                let m = &items[i];
                let entry = model_row(s, m, i, &current, selected, provider, &kit);
                if m.group != Group::Default && (i == 0 || items[i - 1].group != m.group) {
                    col()
                        .w_full()
                        .child(
                            row()
                                .h(GROUP_HEIGHT)
                                .px(6.0)
                                .center_y()
                                .key(format!("model-group-{}", m.group.label()))
                                .child(
                                    text(m.group.label())
                                        .font(kit.ty.small)
                                        .color(kit.c.text_muted),
                                ),
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
        .gap(6.0)
        .child(
            ui::field(&l.models.query, "Search models", "Search models…", k)
                .edit_menu_changed(
                    "model-search",
                    |s| Some(&mut live_mut(s).models.query),
                    |s, tx| {
                        live_mut(s).models.selected = None;
                        live_mut(s).models.reveal = None;
                        tx.scroll_to_y("model-scroll", 0.0);
                    },
                )
                .on_input_key(key)
                .help(
                    "Search models, companies or connections · ↑↓ Navigate · ↵ Choose · Esc Close",
                ),
        )
        .child(filters(s, &all, k));
    if let Some(error) = l.app.notices.get(scope) {
        body = body.child(ui::notice(
            &format!("{} unavailable", ui::provider_name(provider)),
            error,
            k,
        ));
    }
    let body = body
        .child(if count == 0 {
            let favorites =
                l.models.filter == Filter::Favorites && l.models.query.text.trim().is_empty();
            col()
                .w_full()
                .grow(1.0)
                .center()
                .gap(5.0)
                .child(
                    text(if favorites {
                        "No favourite models"
                    } else {
                        "No matching models"
                    })
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
                )
                .child(ui::caption(
                    if favorites {
                        "Star models to keep them here."
                    } else {
                        "Try another search or connection."
                    },
                    k,
                ))
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
        .child(ui::divider(k));
    let mut footer = row()
        .w_full()
        .h(24.0)
        .pl(6.0)
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
        );
    if let Owner::Session(index, scope) = owner {
        let index = *index;
        let scope = scope.clone();
        footer = footer.child(
            ui::Button::new("Save as default")
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
                .h(24.0)
                .px(6.0)
                .radius(5.0)
                .help("Use the current model, effort and agent for new sessions")
                .key("save-provider-defaults"),
        );
    }
    (body.child(footer), height)
}

pub fn overlay(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(owner) = live(s).models.owner.as_ref() else {
        return;
    };
    let defaults = matches!(owner, Owner::Defaults(_));
    let anchor = match owner {
        Owner::Home(_) => "home-model-trigger",
        Owner::Defaults(_) => "settings-model-trigger",
        Owner::Session(..) => "model-trigger",
    };
    let window = rune::memo::window_layout("shell");
    let (body, preferred_height) = view(s, k);
    let mut width = window.map_or(MAX_WIDTH, |r| (r[2] - 32.0).min(MAX_WIDTH));
    let height = window.map_or(preferred_height, |r| (r[3] - 16.0).min(preferred_height));
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
        && trigger[1].max(window[3] - trigger[1] - trigger[3]) < MAX_HEIGHT + 16.0
        && trigger[0] >= MAX_WIDTH + 16.0
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
        .padding(8.0)
        .child(body);
    layers.push(rune_kit::popover::anchored(
        k,
        rune_kit::popover::raised(k, content, 9.0),
        anchor,
        placement,
        8.0,
    ));
}
