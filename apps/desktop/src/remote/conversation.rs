use crate::{
    Workspace,
    context::EditMenu,
    live::{Live, live, live_mut},
    ui,
};
use caelum_core::remote::{Checkout, Job, Message, Speaker, Thread, Turn};
use rune::{El, Key, ScrollSpec, TextEdit, Tx, col, row, text, text_input};
use rune_kit::{ButtonVariant, Kit, ScrollBar};

#[derive(Default)]
pub(super) struct Editors {
    draft: TextEdit,
    recovery: TextEdit,
}

pub(super) fn sync_editor(l: &mut Live) {
    let Some(id) = &l.app.remote.saved.selected else {
        return;
    };
    let Some(c) = l.app.remote.saved.checkouts.get(id) else {
        return;
    };
    let editors = l
        .remote
        .editors
        .entry(c.thread.editor_key(id).to_owned())
        .or_default();
    if editors.draft.text != c.thread.draft {
        editors.draft = TextEdit::new(&c.thread.draft);
    }
}

fn changed(s: &mut Workspace, owner: &str) {
    let l = live_mut(s);
    if let Some(id) = &l.app.remote.saved.selected
        && let Some(c) = l.app.remote.saved.checkouts.get_mut(id)
        && c.thread.editor_key(id) == owner
    {
        c.thread
            .draft
            .clone_from(&l.remote.editors[owner].draft.text);
    }
}
fn send(s: &mut Workspace, tx: &mut Tx, owner: &str) {
    let l = live_mut(s);
    let Some(id) = l.app.remote.saved.selected.clone() else {
        return;
    };
    if l.app.remote.saved.checkouts[&id].thread.editor_key(&id) != owner {
        return;
    }
    let editor = &mut l.remote.editors.get_mut(owner).unwrap().draft;
    if l.app.send_remote(&id, &editor.text) {
        editor.clear();
        editor.focused = true;
        tx.scroll_to_bottom(format!("remote-transcript-{owner}"));
        l.schedule();
    }
}

fn transcript_line(thread: &Thread, index: usize) -> Option<(&str, &str)> {
    let imported = thread
        .continued
        .as_ref()
        .map_or(&[][..], |c| &c.chat.messages);
    if let Some(message) = imported.get(index) {
        return Some(match message {
            Message::User(text) => ("You · local", text),
            Message::Assistant(text) => ("Assistant · local", text),
        });
    }
    let line = thread.lines.get(index - imported.len())?;
    Some((
        match line.speaker {
            Speaker::You => "You",
            Speaker::OpenCode => "OpenCode",
            Speaker::Activity => "Activity",
        },
        &line.text,
    ))
}

pub(super) fn view(s: &Workspace, k: &Kit, id: &str, c: &Checkout, branch: &str) -> El {
    let l = live(s);
    let mut body = col().w_full().h_full().min_h(0.0).items_center().gap(8.0);
    body = body.child(
        row()
            .center_y()
            .gap(8.0)
            .py(8.0)
            .child(
                ui::caption(format!("{}  /  {branch}", c.source.repository), k)
                    .truncate()
                    .min_w(0.0),
            )
            .children((!c.other_threads.is_empty()).then(|| super::threads::button(k))),
    );
    let key = c.thread.editor_key(id);
    let row_key = key.to_owned();
    let owner = id.to_owned();
    let kit = k.clone();
    body = body.child(
        col()
            .key(format!("remote-transcript-{key}"))
            .label("Remote conversation")
            .role("region")
            .w_full()
            .max_w(840.0)
            .min_h(0.0)
            .grow(1.0)
            .px(20.0)
            .scroll(ScrollSpec { stick_bottom: true })
            .child(
                col()
                    .w_full()
                    .growth(rune::GrowthSpec::default())
                    .virtual_rows(
                        c.thread.lines.len()
                            + c.thread
                                .continued
                                .as_ref()
                                .map_or(0, |c| c.chat.messages.len()),
                        80.0,
                        move |_: &Workspace, i| format!("remote-line-{row_key}-{i}").into(),
                        move |s: &Workspace, i| {
                            let Some((speaker, message)) = live(s)
                                .app
                                .remote
                                .saved
                                .checkouts
                                .get(&owner)
                                .and_then(|c| transcript_line(&c.thread, i))
                            else {
                                return col();
                            };
                            col()
                                .w_full()
                                .py(6.0)
                                .gap(4.0)
                                .child(ui::caption(speaker, &kit))
                                .child(
                                    text(message)
                                        .font(kit.ty.ui)
                                        .color(kit.c.text_base)
                                        .selectable(true)
                                        .wrap(),
                                )
                        },
                    ),
            )
            .scroll_bar(k),
    );
    let Some(editors) = l.remote.editors.get(key) else {
        return body;
    };
    let busy = matches!(l.app.remote.feeds.get(id), Some(Job::Busy(_)));
    let mut context = col().w_full().gap(6.0);
    let too_long = editors.draft.text.len() > Thread::MAX_MESSAGE_BYTES;
    if too_long {
        context = context.child(ui::caption(
            "Shorten your message to 64 KiB before sending.",
            k,
        ));
    }
    for error in [l.app.remote.jobs.get(id), l.app.remote.feeds.get(id)]
        .into_iter()
        .flatten()
    {
        if let Job::Failed(e) = error {
            context = context.child(ui::caption(e, k).wrap().color(k.c.danger.fg));
        }
    }
    if c.thread.turn == Turn::Submitting && !busy {
        let a = key.to_owned();
        let recovery = a.clone();
        let b = id.to_owned();
        let d = b.clone();
        context = context
            .child(ui::caption(
                "Delivery unconfirmed. Check Boat, then reconnect with its prompt ID.",
                k,
            ))
            .child(
                ui::field(&editors.recovery, "Boat prompt ID", "Prompt ID", k)
                    .key("remote-recovery")
                    .on_edit(move |s: &mut Workspace| {
                        &mut live_mut(s).remote.editors.get_mut(&a).unwrap().recovery
                    }),
            )
            .child(
                row()
                    .gap(6.0)
                    .child(
                        ui::Button::new("Reconnect turn")
                            .on_press(move |s: &mut Workspace| {
                                let l = live_mut(s);
                                l.app
                                    .recover_remote(&b, &l.remote.editors[&recovery].recovery.text);
                            })
                            .build(k),
                    )
                    .child(
                        ui::Button::new("Dismiss submission")
                            .on_press(move |s: &mut Workspace| {
                                live_mut(s).app.dismiss_remote_submission(&d)
                            })
                            .build(k),
                    ),
            );
    }
    let owner = key.to_owned();
    let change_owner = owner.clone();
    let key_owner = owner.clone();
    let submit_owner = owner.clone();
    let input = text_input(&editors.draft, k.ty.ui)
        .label("Message remote OpenCode")
        .placeholder("Message OpenCode on this machine…", k.c.text_muted)
        .color(k.c.text_base)
        .selection_color(k.selection())
        .w_full()
        .edit_menu_changed(
            format!("remote-draft-{key}"),
            move |s| {
                live_mut(s)
                    .remote
                    .editors
                    .get_mut(&owner)
                    .map(|e| &mut e.draft)
            },
            move |s, _| changed(s, &change_owner),
        )
        .on_input_key(move |s: &mut Workspace, e, tx| {
            if e.key == Key::Enter && !e.mods.shift {
                send(s, tx, &key_owner);
                true
            } else {
                false
            }
        })
        .on_submit(move |s, tx| send(s, tx, &submit_owner));
    let stopped = c
        .machine()
        .is_some_and(|m| matches!(m.state.as_str(), "archived" | "stopped"));
    let stats_owner = id.to_owned();
    let label = format!(
        "Machine stats · {}{}",
        c.title(),
        if stopped { " · Stopped" } else { "" }
    );
    let mut controls = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .children(super::stats::wide().then(|| ui::caption("OpenCode", k)))
        .child(rune::el().grow(1.0))
        .children(super::stats::pinned(s, k, id, c))
        .child(
            crate::quiet_button(&ui::SERVER, &label)
                .variant(rune_kit::IconButtonVariant::Ghost)
                .expanded(l.panels.overlay == Some(crate::panels::Screen::VmStats))
                .on_press(move |s: &mut Workspace| {
                    super::popup::inspect_remote(live_mut(s), &stats_owner)
                })
                .build(k)
                .size(32.0, 32.0)
                .key("remote-badge")
                .help(label),
        );
    if stopped {
        let owner = id.to_owned();
        controls = controls.child(
            ui::Button::new("Resume")
                .on_press(move |s: &mut Workspace| live_mut(s).app.power_remote(&owner, true))
                .build(k),
        );
    } else if matches!(c.thread.turn, Turn::Following(_)) {
        let a = id.to_owned();
        controls = controls.child(
            ui::Button::new("Stop")
                .disabled(l.app.remote.interrupts.contains_key(id))
                .on_press(move |s: &mut Workspace| live_mut(s).app.interrupt_remote(&a))
                .build(k)
                .label("Stop this turn"),
        );
        if matches!(l.app.remote.feeds.get(id), Some(Job::Failed(_))) {
            let a = id.to_owned();
            controls = controls.child(
                ui::Button::new("Reconnect")
                    .on_press(move |s: &mut Workspace| live_mut(s).app.follow_remote(&a))
                    .build(k),
            );
        }
    } else {
        let owner = key.to_owned();
        controls = controls.child(
            ui::Button::new(if c.thread.turn == Turn::Submitting {
                "Sending…"
            } else {
                "Send"
            })
            .variant(ButtonVariant::Contrast)
            .disabled(
                c.thread.turn != Turn::Idle
                    || editors.draft.text.trim().is_empty()
                    || too_long
                    || l.app.remote.jobs.contains_key(id),
            )
            .on_press_tx(move |s, tx| send(s, tx, &owner))
            .build(k)
            .key("remote-send"),
        );
    }
    body.child(ui::composer(
        ui::composer_editor(input, 48.0, 140.0),
        controls,
        context,
        16.0,
        k,
    ))
}
