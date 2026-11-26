use rune::{El, ScrollSpec, TextEdit, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit, Placement, ScrollBar, glyphs};

use crate::live::{live, live_mut};
use crate::ui::Button;
use crate::{Workspace, quiet_button, ui};

use super::label;

const QUEUE_MENU: &str = "queue";

pub fn queue_summary(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let session = &l.app.sessions[index];
    let count = session.queue.len() + session.inbox.items.len();
    if count == 0 {
        return rune::el();
    }
    let failed = session
        .inbox
        .items
        .iter()
        .any(|item| item.error().is_some())
        || (!session.queue.is_empty() && session.error.is_some());
    let paused = session.queue_paused && !session.queue.is_empty();
    let status = if failed {
        " · Needs attention"
    } else if paused {
        " · Paused"
    } else if session.inbox.items.iter().any(|item| item.pending()) {
        " · Saving…"
    } else {
        ""
    };
    let title = format!("{count} queued{status}");
    let open = l.menu == Some((QUEUE_MENU, index));
    let (fill, border, fg, icon_fg) = if failed {
        (
            k.alpha(k.c.danger.fg, 0.12),
            k.alpha(k.c.danger.fg, 0.28),
            k.c.danger.fg,
            k.c.danger.fg,
        )
    } else if paused {
        (
            k.alpha(k.c.warning.fg, 0.12),
            k.alpha(k.c.warning.fg, 0.28),
            k.c.warning.fg,
            k.c.warning.fg,
        )
    } else if open {
        (k.c.hover, k.c.border_strong, k.c.text_base, k.c.text_base)
    } else {
        (
            k.c.layer_01,
            k.c.border_base,
            k.c.text_muted,
            k.c.icon_muted,
        )
    };
    k.interactive(
        row()
            .center_y()
            .gap(5.0)
            .h(24.0)
            .px(8.0)
            .radius(999.0)
            .fill(fill)
            .border(1.0, border)
            .child(icon(&ui::QUEUE, 12.0).color(icon_fg).a11y_hidden())
            .child(
                text(&title)
                    .font(k.ty.small_medium)
                    .color(fg)
                    .truncate()
                    .min_w(0.0),
            )
            .child(ui::disclosure(open, k)),
    )
    .key("toggle-queued-messages")
    .role("button")
    .label(&title)
    .help(if open {
        "Hide queued messages"
    } else {
        "Show queued messages"
    })
    .expanded(open)
    .focusable(true)
    .focus_ring(1.5, 0.0, k.c.border_focus)
    .on_press(move |s: &mut Workspace| {
        let l = live_mut(s);
        let next = Some((QUEUE_MENU, index));
        l.menu = if l.menu == next { None } else { next };
    })
}

pub fn queue_popover(s: &Workspace, k: &Kit, index: usize, layers: &mut Vec<El>) {
    let l = live(s);
    if l.menu != Some((QUEUE_MENU, index)) {
        return;
    }
    let session = &l.app.sessions[index];
    if session.queue.is_empty() && session.inbox.items.is_empty() {
        return;
    }
    let width = rune::memo::window_layout("message-composer")
        .map_or(420.0, |r| (r[2] - 16.0).clamp(280.0, 460.0));
    ui::popup(
        "queued-messages",
        "toggle-queued-messages",
        queued_messages(s, k, index).w(width),
        Placement::TopEnd,
        |s: &mut Workspace| {
            let l = live_mut(s);
            if l.menu.is_some_and(|(name, _)| name == QUEUE_MENU) {
                l.menu = None;
            }
        },
        k,
        layers,
    );
}

pub fn composer_context(s: &Workspace, k: &Kit, index: usize) -> El {
    let mut context = col().w_full().gap(4.0);
    if let Some(error) = live(s).app.persistence_error() {
        context = context.child(label(error, k));
    }
    context.child(crate::composer::attachments(s, k, index))
}

pub(crate) fn queued_preview(text: &str) -> &str {
    text.lines()
        .next()
        .filter(|line| !line.trim().is_empty())
        .unwrap_or("Attached context")
}

fn queued_messages(s: &Workspace, k: &Kit, index: usize) -> El {
    let session = &live(s).app.sessions[index];
    let total = session.queue.len() + session.inbox.items.len();
    let paused = session.queue_paused && !session.queue.is_empty();
    let subtitle = if paused {
        "Paused — resume to send queued messages"
    } else {
        "Runs in order after the current turn"
    };
    let trailing = if paused {
        Some(
            Button::new("Resume")
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Workspace| live_mut(s).app.resume_queue(index))
                .build(k)
                .h(24.0)
                .px(8.0)
                .label("Resume queued messages")
                .key("resume-queued-messages"),
        )
    } else if !session.queue.is_empty() {
        Some(
            Button::new("Pause")
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Workspace| {
                    live_mut(s).app.sessions[index].queue_paused = true;
                })
                .build(k)
                .h(24.0)
                .px(8.0)
                .label("Pause queue")
                .key("pause-queued-messages"),
        )
    } else {
        None
    };

    let mut list = col().w_full().gap(4.0);
    for item in &session.inbox.items {
        let id = item.id.clone();
        let retry = id.clone();
        let mut entry = row()
            .w_full()
            .gap(4.0)
            .px(4.0)
            .center_y()
            .child(
                label(
                    format!(
                        "{}{}",
                        if item.pending() {
                            "Saving… "
                        } else {
                            "Queued: "
                        },
                        queued_preview(&item.text)
                    ),
                    k,
                )
                .help(queued_preview(&item.text))
                .truncate()
                .grow(1.0)
                .min_w(0.0),
            )
            .child(
                Button::new("Remove")
                    .variant(ButtonVariant::GhostMuted)
                    .disabled(item.pending())
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        l.app.remove_queued(index, &id);
                        let session = &l.app.sessions[index];
                        if session.queue.is_empty() && session.inbox.items.is_empty() {
                            l.menu = None;
                        }
                    })
                    .build(k)
                    .h(24.0)
                    .px(6.0)
                    .key(format!("remove-queued-{}", item.id)),
            );
        let mut item_view = col().w_full().gap(4.0);
        if let Some(error) = item.error() {
            entry = entry.child(
                Button::new("Retry")
                    .variant(ButtonVariant::Outline)
                    .disabled(item.pending())
                    .on_press(move |s: &mut Workspace| live_mut(s).app.retry_queued(index, &retry))
                    .build(k)
                    .h(24.0)
                    .px(8.0)
                    .key(format!("retry-queued-{}", item.id)),
            );
            item_view = item_view.child(label(error, k).color(k.c.danger.fg).px(4.0).wrap());
        }
        list = list.child(item_view.child(entry));
    }
    if !session.queue.is_empty()
        && let Some(error) = &session.error
    {
        list = list.child(label(error, k).color(k.c.danger.fg).px(4.0).wrap());
    }
    let count = session.queue.len();
    for (position, queued) in session.queue.iter().enumerate() {
        list = list.child(
            row()
                .w_full()
                .center_y()
                .gap(4.0)
                .px(4.0)
                .py(2.0)
                .radius(6.0)
                .child(
                    text(format!("{}", position + 1))
                        .font(k.ty.mono)
                        .color(k.c.text_faint)
                        .w(16.0),
                )
                .child(
                    text(queued_preview(&queued.text))
                        .font(k.ty.small)
                        .color(k.c.text_base)
                        .help(queued_preview(&queued.text))
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(
                    quiet_button(&glyphs::CHEVRON_UP, "Move queued message earlier")
                        .disabled(position == 0)
                        .on_press(move |s: &mut Workspace| {
                            let queue = &mut live_mut(s).app.sessions[index].queue;
                            if position > 0 && position < queue.len() {
                                queue.swap(position, position - 1);
                            }
                        })
                        .build(k)
                        .key(format!("move-queued-{index}-{position}")),
                )
                .child(
                    quiet_button(&glyphs::CHEVRON_DOWN, "Move queued message later")
                        .disabled(position + 1 >= count)
                        .on_press(move |s: &mut Workspace| {
                            let queue = &mut live_mut(s).app.sessions[index].queue;
                            if position + 1 < queue.len() {
                                queue.swap(position, position + 1);
                            }
                        })
                        .build(k)
                        .key(format!("move-queued-down-{index}-{position}")),
                )
                .child(
                    Button::new("Edit")
                        .variant(ButtonVariant::GhostMuted)
                        .disabled(
                            !live(s).drafts[index].text.is_empty()
                                || !session.attachments.is_empty()
                                || session.attaching > 0,
                        )
                        .on_press(move |s: &mut Workspace| {
                            let live = live_mut(s);
                            if position < live.app.sessions[index].queue.len()
                                && live.drafts[index].text.is_empty()
                                && live.app.sessions[index].attachments.is_empty()
                            {
                                let queued = live.app.sessions[index].queue.remove(position);
                                let (text, attachments) =
                                    caelum_core::application::split_context(&queued.text);
                                live.drafts[index] = TextEdit::new(text);
                                live.app.sessions[index].attachments = attachments;
                                live.app.sessions[index].options = queued.options;
                                live.app.options_changed(index);
                                live.menu = None;
                                live.drafts[index].focused = true;
                            }
                        })
                        .build(k)
                        .h(24.0)
                        .px(6.0)
                        .key(format!("edit-queued-{index}-{position}")),
                )
                .child(
                    Button::new("Remove")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Workspace| {
                            let live = live_mut(s);
                            let queue = &mut live.app.sessions[index].queue;
                            if position < queue.len() {
                                queue.remove(position);
                            }
                            let session = &live.app.sessions[index];
                            if session.queue.is_empty() && session.inbox.items.is_empty() {
                                live.menu = None;
                            }
                        })
                        .build(k)
                        .h(24.0)
                        .px(6.0)
                        .key(format!("remove-local-queued-{index}-{position}")),
                ),
        );
    }

    col()
        .w_full()
        .gap(6.0)
        .child(ui::popup_header(
            format!("Queued · {total}"),
            Some(subtitle),
            trailing,
            k,
        ))
        .child(
            list.max_h(260.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .scroll_bar(k),
        )
}
