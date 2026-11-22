use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    panels::Screen,
    ui::{self, Button},
};
use caelum_core::application::EntryKind;
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row, text};
use rune_kit::{ButtonVariant, Kit};

pub(crate) fn open(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(index) = l.app.selected else { return };
    l.commands.owner = Some(index);
    l.commands.search.clear();
    l.commands.timeline_selected = l.current_prompt.clone();
    l.commands.timeline_current = l.current_prompt.clone();
    l.panels.set_overlay(Some(Screen::Timeline));
    l.commands.search.focused = true;
}

pub(super) fn timeline(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let session = &l.app.sessions[index];
    let query = l.commands.search.text.to_lowercase();
    let matches: Vec<_> = session
        .conversation
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.kind() == EntryKind::User && caelum_core::application::presentation::entry_visible(e)
        })
        .enumerate()
        .filter(|(_, (_, e))| {
            caelum_core::application::presentation::entry_text(e)
                .to_lowercase()
                .contains(&query)
        })
        .map(|(ordinal, (i, _))| (ordinal + 1, i))
        .collect();
    let kit = k.clone();
    let ids = matches.clone();
    let count = matches.len();
    let rows = col()
        .key("timeline-rows")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            count,
            82.0,
            move |s: &Workspace, i| {
                format!(
                    "timeline-{}",
                    live(s).app.sessions[index].conversation.entries[ids[i].1].id
                )
                .into()
            },
            move |s: &Workspace, i| {
                let (ordinal, position) = matches[i];
                let entry = &live(s).app.sessions[index].conversation.entries[position];
                let id = entry.id.clone();
                let current = live(s).commands.timeline_current.as_ref() == Some(&id);
                let selected = live(s).commands.timeline_selected.as_ref() == Some(&id);
                let shown = caelum_core::application::presentation::entry_text(entry);
                let excerpt = shown
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("Attached context")
                    .trim()
                    .to_owned();
                let detail = shown
                    .lines()
                    .skip(1)
                    .filter(|l| !l.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let highlights = |value: &str| {
                    caelum_core::application::presentation::search_ranges(
                        value,
                        &live(s).commands.search.text,
                    )
                    .into_iter()
                    .map(|r| {
                        rune::Span::new(r.start, r.end)
                            .color(kit.c.text_accent)
                            .weight(600.0)
                            .bg(kit.c.layer_03, 1.0)
                    })
                    .collect::<Vec<_>>()
                };
                let group =
                    |e: &caelum_core::application::Entry| e.started_at.map(|at| at / 86_400_000);
                let day = group(entry);
                let start = i == 0
                    || group(&live(s).app.sessions[index].conversation.entries[matches[i - 1].1])
                        != day;
                let day_label = day.map_or_else(
                    || "This session".into(),
                    |day| match (caelum_core::application::presentation::now() / 86_400_000)
                        .saturating_sub(day)
                    {
                        0 => "Today".into(),
                        1 => "Yesterday".into(),
                        n => format!("{n} days ago"),
                    },
                );
                let mut item = col().w_full().gap(5.0);
                if start {
                    item = item.child(ui::caption(day_label, &kit).px(10.0).pt(8.0));
                }
                item.child(
                    kit.interactive(
                        row()
                            .w_full()
                            .min_h(62.0)
                            .padding(10.0)
                            .gap(12.0)
                            .radius(8.0)
                            .fill(if selected {
                                kit.c.hover
                            } else {
                                rune::Color::TRANSPARENT
                            })
                            .child(text(format!("{ordinal:02}")).font(kit.ty.mono).color(
                                if current {
                                    kit.c.text_accent
                                } else {
                                    kit.c.text_faint
                                },
                            ))
                            .child(
                                col()
                                    .grow(1.0)
                                    .min_w(0.0)
                                    .gap(4.0)
                                    .child(
                                        row()
                                            .w_full()
                                            .center_y()
                                            .gap(8.0)
                                            .child(
                                                text(&excerpt)
                                                    .spans(highlights(&excerpt))
                                                    .font(kit.ty.ui_medium)
                                                    .color(kit.c.text_base)
                                                    .truncate()
                                                    .grow(1.0)
                                                    .min_w(0.0),
                                            )
                                            .children(current.then(|| {
                                                text("Reading here")
                                                    .font(kit.ty.small)
                                                    .color(kit.c.text_accent)
                                                    .px(6.0)
                                                    .py(1.0)
                                                    .radius(4.0)
                                                    .fill(kit.c.layer_02)
                                            })),
                                    )
                                    .children((!detail.is_empty()).then(|| {
                                        ui::caption(&detail, &kit)
                                            .spans(highlights(&detail))
                                            .truncate()
                                    })),
                            ),
                    )
                    .role("button")
                    .label(&excerpt)
                    .description(if current {
                        "Current reading position"
                    } else {
                        "Reveal in conversation"
                    })
                    .focusable(true)
                    .focus_ring(1.5, 0.0, kit.c.border_focus)
                    .on_press_tx(move |s: &mut Workspace, tx| {
                        jump(s, index, &id, tx);
                    }),
                )
            },
        );
    let mut body = col()
        .w_full()
        .h_full()
        .gap(10.0)
        .min_h(0.0)
        .child(
            ui::field(&l.commands.search, "Find a prompt", "Find a prompt…", k)
                .edit_menu_changed(
                    "timeline-search",
                    |s| Some(&mut live_mut(s).commands.search),
                    |s, tx| {
                        live_mut(s).commands.timeline_selected = None;
                        tx.scroll_to_y("timeline-scroll", 0.0);
                    },
                )
                .on_input_key(timeline_key),
        )
        .child(
            col()
                .key("timeline-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        );
    if count == 0 {
        body = body.child(ui::empty(
            "No matching prompts",
            "Try another search or load earlier history.",
            k,
        ));
    }
    if let Some(error) = &session.error {
        body = body.child(ui::caption(error, k).color(k.c.danger.fg));
    }
    let mut footer = row().w_full().center_y().justify_between().gap(8.0).child(
        ui::caption(
            format!(
                "{count} {} · ↑↓ Navigate · ↵ Jump · Esc Close",
                if count == 1 { "prompt" } else { "prompts" },
            ),
            k,
        )
        .truncate()
        .grow(1.0)
        .min_w(0.0),
    );
    if session.next_cursor().is_some() {
        footer = footer.child(
            Button::new(if session.loading() {
                "Loading…"
            } else {
                "Load earlier prompts"
            })
            .variant(ButtonVariant::Outline)
            .disabled(session.loading())
            .on_press(move |s: &mut Workspace| live_mut(s).app.older(index))
            .build(k),
        );
    }
    body.child(ui::divider(k)).child(footer)
}

fn jump(s: &mut Workspace, index: usize, id: &str, tx: &mut Tx) {
    let l = live_mut(s);
    let session = &l.app.sessions[index];
    let Some(entry) = session.conversation.entries.iter().find(|e| e.id == id) else {
        return;
    };
    let target = format!("live-entry-{index}-{}", session.transcript_id(entry));
    l.app.select(index);
    l.current_prompt = Some(id.into());
    l.panels.set_overlay(None);
    l.panels.screen = Screen::Conversation;
    l.commands.search.focused = false;
    tx.scroll_center_in_instant(format!("live-rows-{index}"), target);
}

pub fn timeline_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let l = live(s);
    let Some(index) = l.commands.owner else {
        return false;
    };
    let query = l.commands.search.text.to_lowercase();
    let ids: Vec<_> = l.app.sessions[index]
        .conversation
        .entries
        .iter()
        .filter(|e| {
            e.kind() == EntryKind::User
                && caelum_core::application::presentation::entry_visible(e)
                && caelum_core::application::presentation::entry_text(e)
                    .to_lowercase()
                    .contains(&query)
        })
        .map(|e| e.id.clone())
        .collect();
    let current = ids
        .iter()
        .position(|id| Some(id) == l.commands.timeline_selected.as_ref());
    let at = match ev.key {
        Key::Down => current.map_or(0, |i| (i + 1).min(ids.len().saturating_sub(1))),
        Key::Up => current.unwrap_or(0).saturating_sub(1),
        Key::Enter => {
            if let Some(id) = ids.get(current.unwrap_or(0)) {
                jump(s, index, id, tx);
            }
            return true;
        }
        _ => return false,
    };
    live_mut(s).commands.timeline_selected = ids.get(at).cloned();
    live_mut(s).commands.search.focused = true;
    if let Some(id) = ids.get(at) {
        tx.scroll_center_in_instant("timeline-rows", format!("timeline-{id}"));
    }
    true
}
