use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    panels::Screen,
    quiet_button,
    ui::{self, Button},
};
use caelum_core::application::EntryKind;
use rune::{El, Key, KeyEvent, ScrollSpec, TextStyle, Tx, col, icon, row, text, text_input};
use rune_kit::{ButtonVariant, Kit, ScrollBar, glyphs};

mod entries;

use entries::{DAY_MS, day_bucket, matching_turns};

pub(crate) fn open(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(index) = l.app.selected else { return };
    l.commands.owner = Some(index);
    l.commands.search.clear();
    l.commands.timeline_bookmarked_only = false;
    l.commands.timeline_selected = l.current_prompt.clone();
    l.commands.timeline_current = l.current_prompt.clone();
    l.panels.set_overlay(Some(Screen::Timeline));
    l.commands.search.focused = true;
}

fn day_heading(day: Option<u64>, now: u64) -> String {
    day.map_or_else(
        || "This session".into(),
        |day| match (now / DAY_MS).saturating_sub(day) {
            0 => "Today".into(),
            1 => "Yesterday".into(),
            n => format!("{n} days ago"),
        },
    )
}

fn relative_time(at: Option<u64>, now: u64) -> String {
    let Some(at) = at else {
        return String::new();
    };
    let mins = now.saturating_sub(at) / 60_000;
    if mins < 1 {
        "now".into()
    } else if mins < 60 {
        format!("{mins}m ago")
    } else if mins < 1_440 {
        format!("{}h ago", mins / 60)
    } else {
        format!("{}d ago", mins / 1_440)
    }
}

fn keycap(label: &str, k: &Kit) -> El {
    text(label)
        .font(k.ty.small)
        .color(k.c.text_muted)
        .px(5.0)
        .py(1.0)
        .radius(4.0)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_base)
        .a11y_hidden()
}

fn key_hint(keys: &str, action: &str, k: &Kit) -> El {
    row()
        .center_y()
        .gap(5.0)
        .child(keycap(keys, k))
        .child(text(action).font(k.ty.small).color(k.c.text_faint))
}

fn meta_chip(glyph: &'static rune::Icon, label: String, color: rune::Color) -> El {
    row()
        .center_y()
        .gap(4.0)
        .child(icon(glyph, 11.0).color(color).a11y_hidden())
        .child(text(label).font(TextStyle::new(11.0, 450.0)).color(color))
}

fn status_badge(label: &str, fg: rune::Color, k: &Kit) -> El {
    text(label)
        .font(TextStyle::new(10.5, 550.0))
        .color(fg)
        .px(6.0)
        .py(1.0)
        .radius(4.0)
        .fill(k.alpha(fg, 0.12))
        .border(1.0, k.alpha(fg, 0.24))
}

fn search_bar(s: &Workspace, count: usize, total: usize, k: &Kit) -> El {
    let l = live(s);
    let filtering = !l.commands.search.text.is_empty();
    row()
        .w_full()
        .min_h(36.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(k.c.bg_base)
        .border(
            1.0,
            if l.commands.search.focused {
                k.c.border_focus
            } else {
                k.c.border_base
            },
        )
        .child(
            icon(&glyphs::SEARCH, 14.0)
                .color(if l.commands.search.focused {
                    k.c.text_accent
                } else {
                    k.c.icon_muted
                })
                .a11y_hidden(),
        )
        .child(
            text_input(&l.commands.search, k.ty.ui)
                .single_line()
                .label("Find a prompt")
                .placeholder("Find a prompt…", k.c.text_muted)
                .color(k.c.text_base)
                .selection_color(k.selection())
                .grow(1.0)
                .min_w(0.0)
                .h(32.0)
                .py(6.0)
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
        .children(filtering.then(|| {
            text(format!("{count} / {total}"))
                .font(k.ty.mono)
                .color(k.c.text_faint)
                .px(6.0)
                .py(2.0)
                .radius(4.0)
                .fill(k.c.layer_02)
        }))
        .children(filtering.then(|| {
            quiet_button(&glyphs::CLOSE, "Clear search")
                .on_press_tx(|s: &mut Workspace, tx: &mut Tx| {
                    let l = live_mut(s);
                    l.commands.search.clear();
                    l.commands.timeline_selected = l.commands.timeline_current.clone();
                    l.commands.search.focused = true;
                    tx.scroll_to_y("timeline-scroll", 0.0);
                })
                .build(k)
        }))
}

fn rail_node(current: bool, selected: bool, failed: bool, running: bool, k: &Kit) -> El {
    if current {
        row()
            .size(14.0, 14.0)
            .radius(7.0)
            .center()
            .fill(k.alpha(k.c.text_accent, 0.22))
            .pin(Some(5.0), Some(12.0), None, None)
            .a11y_hidden()
            .child(rune::el().size(8.0, 8.0).radius(4.0).fill(k.c.text_accent))
    } else if failed {
        rune::el()
            .size(9.0, 9.0)
            .radius(4.5)
            .fill(k.c.danger.fg)
            .border(1.5, k.c.layer_01)
            .pin(Some(7.5), Some(14.5), None, None)
            .a11y_hidden()
    } else if running {
        row()
            .size(12.0, 12.0)
            .radius(6.0)
            .center()
            .fill(k.alpha(k.c.text_accent, 0.16))
            .pin(Some(6.0), Some(13.0), None, None)
            .a11y_hidden()
            .child(rune::el().size(6.0, 6.0).radius(3.0).fill(k.c.text_accent))
    } else if selected {
        rune::el()
            .size(9.0, 9.0)
            .radius(4.5)
            .fill(k.c.text_base)
            .border(1.5, k.c.layer_01)
            .pin(Some(7.5), Some(14.5), None, None)
            .a11y_hidden()
    } else {
        rune::el()
            .size(8.0, 8.0)
            .radius(4.0)
            .fill(k.c.layer_01)
            .border(1.5, k.c.border_strong)
            .pin(Some(8.0), Some(15.0), None, None)
            .a11y_hidden()
    }
}

pub(super) fn timeline(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let session = &l.app.sessions[index];
    let query = l.commands.search.text.to_lowercase();
    let bookmarked_only = l.commands.timeline_bookmarked_only;
    let total = matching_turns(session, "", bookmarked_only).len();
    let bookmarked_count = matching_turns(session, "", true).len();
    let matches = matching_turns(session, &query, bookmarked_only);
    let count = matches.len();
    let active_index = matches
        .iter()
        .position(|turn| {
            Some(&session.conversation.entries[turn.target_position].id)
                == l.commands.timeline_selected.as_ref()
        })
        .unwrap_or(0);

    let mut group_counts = vec![0usize; count];
    let mut group_start = 0;
    while group_start < count {
        let day = day_bucket(&session.conversation.entries[matches[group_start].target_position]);
        let mut group_end = group_start + 1;
        while group_end < count
            && day_bucket(&session.conversation.entries[matches[group_end].target_position]) == day
        {
            group_end += 1;
        }
        group_counts[group_start] = group_end - group_start;
        group_start = group_end;
    }

    let now = caelum_core::application::presentation::now();
    let kit = k.clone();
    let ids = matches.clone();
    let rows = col()
        .key("timeline-rows")
        .w_full()
        .growth(rune::GrowthSpec::default())
        .role("list")
        .label("Prompt timeline")
        .virtual_rows(
            count,
            88.0,
            move |s: &Workspace, i| {
                format!(
                    "timeline-{}",
                    live(s).app.sessions[index].conversation.entries[ids[i].target_position].id
                )
                .into()
            },
            move |s: &Workspace, i| {
                let turn = matches[i];
                let entries = &live(s).app.sessions[index].conversation.entries;
                let entry = &entries[turn.target_position];
                let id = entry.id.clone();
                let current = live(s).commands.timeline_current.as_ref() == Some(&id);
                let selected = i == active_index;
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
                let day = day_bucket(entry);
                let start = i == 0 || day_bucket(&entries[matches[i - 1].target_position]) != day;
                let end_of_group = i + 1 == matches.len()
                    || day_bucket(&entries[matches[i + 1].target_position]) != day;
                let age = relative_time(entry.started_at, now);

                let mut item = col().w_full();
                if start {
                    let group_count = group_counts[i];
                    item = item.child(
                        row()
                            .w_full()
                            .center_y()
                            .gap(8.0)
                            .pl(6.0)
                            .pr(4.0)
                            .pt(if i == 0 { 2.0 } else { 12.0 })
                            .pb(6.0)
                            .child(
                                text(day_heading(day, now))
                                    .font(TextStyle::new(11.0, 550.0))
                                    .color(kit.c.text_muted),
                            )
                            .child(
                                text(group_count.to_string())
                                    .font(TextStyle::new(10.5, 500.0))
                                    .color(kit.c.text_muted)
                                    .px(6.0)
                                    .py(1.0)
                                    .radius(9.0)
                                    .fill(kit.alpha(kit.c.text_muted, 0.12)),
                            )
                            .child(
                                rune::el()
                                    .grow(1.0)
                                    .h(1.0)
                                    .fill(kit.c.border_base)
                                    .opacity(0.65),
                            ),
                    );
                }

                let has_meta = turn.tools > 0
                    || turn.attachments > 0
                    || turn.failed
                    || turn.running
                    || turn.compacted;
                let mut meta = row().w_full().center_y().gap(10.0).pt(2.0);
                if turn.tools > 0 {
                    meta = meta.child(meta_chip(
                        &ui::TOOL,
                        format!(
                            "{} {}",
                            turn.tools,
                            if turn.tools == 1 { "tool" } else { "tools" }
                        ),
                        kit.c.text_faint,
                    ));
                }
                if turn.attachments > 0 {
                    meta = meta.child(meta_chip(
                        &ui::FILE,
                        format!(
                            "{} {}",
                            turn.attachments,
                            if turn.attachments == 1 {
                                "attachment"
                            } else {
                                "attachments"
                            }
                        ),
                        kit.c.text_faint,
                    ));
                }
                if turn.compacted {
                    meta = meta.child(status_badge("Compacted", kit.c.text_muted, &kit));
                }
                if turn.running {
                    meta = meta.child(status_badge("Running", kit.c.text_accent, &kit));
                } else if turn.failed {
                    meta = meta.child(status_badge("Error", kit.c.danger.fg, &kit));
                }

                let card_fill = if selected {
                    kit.c.hover
                } else if current {
                    kit.alpha(kit.c.text_accent, 0.06)
                } else {
                    rune::Color::TRANSPARENT
                };
                let card_border = if current {
                    kit.alpha(kit.c.text_accent, if selected { 0.38 } else { 0.24 })
                } else if selected {
                    kit.c.border_base
                } else {
                    rune::Color::TRANSPARENT
                };

                let card = kit
                    .interactive(
                        col()
                            .w_full()
                            .px(12.0)
                            .py(9.0)
                            .gap(4.0)
                            .radius(8.0)
                            .fill(card_fill)
                            .border(1.0, card_border)
                            .child(
                                row()
                                    .w_full()
                                    .center_y()
                                    .gap(8.0)
                                    .child(
                                        text(format!("{:02}", turn.ordinal))
                                            .font(kit.ty.mono)
                                            .color(if current {
                                                kit.c.text_accent
                                            } else {
                                                kit.c.text_faint
                                            }),
                                    )
                                    .child(
                                        text(&excerpt)
                                            .spans(highlights(&excerpt))
                                            .font(kit.ty.ui_medium)
                                            .color(kit.c.text_base)
                                            .truncate()
                                            .grow(1.0)
                                            .min_w(0.0),
                                    )
                                    .children((entry.kind() == EntryKind::Assistant).then(|| {
                                        text("Reply")
                                            .font(kit.ty.small)
                                            .color(kit.c.text_muted)
                                            .px(6.0)
                                            .py(1.0)
                                            .radius(4.0)
                                            .fill(kit.c.layer_02)
                                    }))
                                    .children(live(s).app.sessions[index].is_bookmarked(&id).then(
                                        || {
                                            icon(&ui::BOOKMARK, 11.0)
                                                .color(kit.c.text_accent)
                                                .a11y_hidden()
                                        },
                                    ))
                                    .children(current.then(|| {
                                        text("Reading here")
                                            .font(TextStyle::new(10.5, 550.0))
                                            .color(kit.c.text_accent)
                                            .px(6.0)
                                            .py(1.0)
                                            .radius(4.0)
                                            .fill(kit.alpha(kit.c.text_accent, 0.14))
                                            .border(1.0, kit.alpha(kit.c.text_accent, 0.28))
                                    }))
                                    .children((!age.is_empty()).then(|| {
                                        text(&age).font(kit.ty.small).color(kit.c.text_faint)
                                    }))
                                    .children(selected.then(|| {
                                        text("↵")
                                            .font(kit.ty.small)
                                            .color(kit.c.text_muted)
                                            .a11y_hidden()
                                    })),
                            )
                            .children((!detail.is_empty()).then(|| {
                                ui::caption(&detail, &kit)
                                    .spans(highlights(&detail))
                                    .truncate()
                            }))
                            .children(has_meta.then_some(meta)),
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
                    });

                let mut entry_row = col().w_full().pl(26.0).pr(2.0).pb(4.0);
                if !start && !end_of_group {
                    entry_row = entry_row.child(
                        rune::el()
                            .w(1.5)
                            .fill(kit.c.border_base)
                            .pin(Some(11.25), Some(0.0), None, Some(0.0))
                            .a11y_hidden(),
                    );
                } else if start && !end_of_group {
                    entry_row = entry_row.child(
                        rune::el()
                            .w(1.5)
                            .fill(kit.c.border_base)
                            .pin(Some(11.25), Some(19.0), None, Some(0.0))
                            .a11y_hidden(),
                    );
                } else if !start && end_of_group {
                    entry_row = entry_row.child(
                        rune::el()
                            .w(1.5)
                            .h(19.0)
                            .fill(kit.c.border_base)
                            .pin(Some(11.25), Some(0.0), None, None)
                            .a11y_hidden(),
                    );
                }
                entry_row = entry_row
                    .child(rail_node(
                        current,
                        selected,
                        turn.failed,
                        turn.running,
                        &kit,
                    ))
                    .child(card);

                item.child(entry_row)
            },
        );

    let mut body = col()
        .w_full()
        .h_full()
        .gap(10.0)
        .min_h(0.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(6.0)
                .child(
                    Button::new("All prompts")
                        .variant(if bookmarked_only {
                            ButtonVariant::GhostMuted
                        } else {
                            ButtonVariant::Outline
                        })
                        .on_press_tx(|s: &mut Workspace, tx| {
                            let l = live_mut(s);
                            l.commands.timeline_bookmarked_only = false;
                            l.commands.timeline_selected = None;
                            tx.scroll_to_y("timeline-scroll", 0.0);
                        })
                        .build(k)
                        .h(26.0)
                        .px(9.0)
                        .key("timeline-filter-all"),
                )
                .child(
                    Button::new(if bookmarked_count > 0 {
                        format!("Bookmarked ({bookmarked_count})")
                    } else {
                        "Bookmarked".into()
                    })
                    .icon(&ui::BOOKMARK)
                    .variant(if bookmarked_only {
                        ButtonVariant::Outline
                    } else {
                        ButtonVariant::GhostMuted
                    })
                    .on_press_tx(|s: &mut Workspace, tx| {
                        let l = live_mut(s);
                        l.commands.timeline_bookmarked_only = true;
                        l.commands.timeline_selected = None;
                        tx.scroll_to_y("timeline-scroll", 0.0);
                    })
                    .build(k)
                    .h(26.0)
                    .px(9.0)
                    .key("timeline-filter-bookmarked"),
                ),
        )
        .child(search_bar(s, count, total, k));

    if count == 0 {
        let filtered = !l.commands.search.text.is_empty();
        let (title, detail) = if bookmarked_only && !filtered {
            (
                "No bookmarked messages",
                "Bookmark a prompt or reply to save it here.",
            )
        } else if filtered && bookmarked_only {
            (
                "No matching bookmarks",
                "Try another search or clear the filter.",
            )
        } else if filtered {
            (
                "No matching prompts",
                "Try another search or clear the filter.",
            )
        } else {
            (
                "No prompts yet",
                "Prompts you send in this session will appear on the timeline.",
            )
        };
        let mut empty_state = col()
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .center()
            .gap(8.0)
            .child(ui::empty(title, detail, k));
        if filtered {
            empty_state = empty_state.child(
                Button::new("Clear search")
                    .variant(ButtonVariant::Outline)
                    .on_press(|s: &mut Workspace| {
                        let l = live_mut(s);
                        l.commands.search.clear();
                        l.commands.timeline_selected = l.commands.timeline_current.clone();
                        l.commands.search.focused = true;
                    })
                    .build(k),
            );
        }
        body = body.child(empty_state);
    } else {
        body = body.child(
            col()
                .key("timeline-scroll")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows)
                .scroll_bar(k),
        );
    }

    if let Some(error) = &session.error {
        body = body.child(ui::caption(error, k).color(k.c.danger.fg));
    }

    let noun = if bookmarked_only {
        if count == 1 { "message" } else { "messages" }
    } else {
        if count == 1 { "prompt" } else { "prompts" }
    };
    let summary = if count == total {
        format!("{count} {noun}")
    } else {
        format!("{count} of {total} {noun}")
    };
    let mut trailing = row()
        .center_y()
        .gap(12.0)
        .child(key_hint("↑↓", "Navigate", k))
        .child(key_hint("↵", "Jump", k))
        .child(key_hint("Esc", "Close", k));
    if session.next_cursor().is_some() {
        trailing = trailing.child(
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
    let footer = row()
        .w_full()
        .center_y()
        .justify_between()
        .gap(8.0)
        .child(ui::caption(summary, k).truncate().grow(1.0).min_w(0.0))
        .child(trailing);
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
    tx.scroll_center_in_instant(format!("live-transcript-{index}"), target);
}

pub fn timeline_key(s: &mut Workspace, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let l = live(s);
    let Some(index) = l.commands.owner else {
        return false;
    };
    let query = l.commands.search.text.to_lowercase();
    let ids: Vec<_> = matching_turns(
        &l.app.sessions[index],
        &query,
        l.commands.timeline_bookmarked_only,
    )
    .into_iter()
    .map(|turn| {
        l.app.sessions[index].conversation.entries[turn.target_position]
            .id
            .clone()
    })
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
        tx.scroll_center_in_instant("timeline-scroll", format!("timeline-{id}"));
    }
    true
}
