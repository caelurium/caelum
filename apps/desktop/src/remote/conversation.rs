use crate::{
    Workspace,
    context::EditMenu,
    live::{Live, live, live_mut},
    ui,
};
use caelum_core::remote::{Checkout, Job, Speaker, Turn};
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
    let editors = l.remote.editors.entry(id.clone()).or_default();
    if editors.draft.text != c.thread.draft {
        editors.draft = TextEdit::new(&c.thread.draft);
    }
}

fn changed(s: &mut Workspace, _: &mut Tx) {
    let l = live_mut(s);
    if let Some(id) = &l.app.remote.saved.selected
        && let Some(c) = l.app.remote.saved.checkouts.get_mut(id)
    {
        c.thread.draft.clone_from(&l.remote.editors[id].draft.text);
    }
}
fn send(s: &mut Workspace, tx: &mut Tx) {
    let l = live_mut(s);
    let Some(id) = l.app.remote.saved.selected.clone() else {
        return;
    };
    let editor = &mut l.remote.editors.get_mut(&id).unwrap().draft;
    if l.app.send_remote(&id, &editor.text) {
        editor.clear();
        editor.focused = true;
        tx.scroll_to_bottom(format!("remote-transcript-{id}"));
        l.schedule();
    }
}

pub(super) fn view(s: &Workspace, k: &Kit, id: &str, c: &Checkout, branch: &str) -> El {
    let l = live(s);
    let mut body = col().w_full().h_full().min_h(0.0).items_center().gap(8.0);
    body = body.child(ui::caption(format!("{}  /  {branch}", c.source.repository), k).py(12.0));
    let key = id.to_owned();
    let owner = id.to_owned();
    let kit = k.clone();
    body = body.child(
        col()
            .key(format!("remote-transcript-{id}"))
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
                        c.thread.lines.len(),
                        80.0,
                        move |_: &Workspace, i| format!("remote-line-{key}-{i}").into(),
                        move |s: &Workspace, i| {
                            let Some(line) = live(s)
                                .app
                                .remote
                                .saved
                                .checkouts
                                .get(&owner)
                                .and_then(|c| c.thread.lines.get(i))
                            else {
                                return col();
                            };
                            col()
                                .w_full()
                                .py(6.0)
                                .gap(4.0)
                                .child(ui::caption(
                                    match line.speaker {
                                        Speaker::You => "You",
                                        Speaker::OpenCode => "OpenCode",
                                        Speaker::Activity => "Activity",
                                    },
                                    &kit,
                                ))
                                .child(
                                    text(&line.text)
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
    let Some(editors) = l.remote.editors.get(id) else {
        return body;
    };
    let busy = matches!(l.app.remote.feeds.get(id), Some(Job::Busy(_)));
    let mut context = col().w_full().gap(6.0);
    for error in [l.app.remote.jobs.get(id), l.app.remote.feeds.get(id)]
        .into_iter()
        .flatten()
    {
        if let Job::Failed(e) = error {
            context = context.child(ui::caption(e, k).wrap().color(k.c.danger.fg));
        }
    }
    if c.thread.turn == Turn::Submitting && !busy {
        let a = id.to_owned();
        let b = a.clone();
        let d = a.clone();
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
                                    .recover_remote(&b, &l.remote.editors[&b].recovery.text);
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
    if l.remote.details {
        context = context.child(details(s, k, id, c));
    }
    let owner = id.to_owned();
    let input = text_input(&editors.draft, k.ty.ui)
        .label("Message remote OpenCode")
        .placeholder("Message OpenCode on this machine…", k.c.text_muted)
        .color(k.c.text_base)
        .selection_color(k.selection())
        .w_full()
        .edit_menu_changed(
            format!("remote-draft-{id}"),
            move |s| {
                live_mut(s)
                    .remote
                    .editors
                    .get_mut(&owner)
                    .map(|e| &mut e.draft)
            },
            changed,
        )
        .on_input_key(|s: &mut Workspace, e, tx| {
            if e.key == Key::Enter && !e.mods.shift {
                send(s, tx);
                true
            } else {
                false
            }
        })
        .on_submit(send);
    let mut controls = row()
        .w_full()
        .gap(6.0)
        .center_y()
        .child(
            ui::Button::new(format!("Remote · {}", c.title()))
                .variant(ButtonVariant::GhostMuted)
                .on_press(|s: &mut Workspace| live_mut(s).remote.details = !live(s).remote.details)
                .build(k)
                .key("remote-badge")
                .min_w(0.0)
                .shrink(),
        )
        .child(rune::el().grow(1.0));
    if matches!(c.thread.turn, Turn::Following(_)) {
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
                    || l.app.remote.jobs.contains_key(id),
            )
            .on_press_tx(send)
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

fn details(_s: &Workspace, k: &Kit, id: &str, c: &Checkout) -> El {
    let a = id.to_owned();
    let b = a.clone();
    col()
        .w_full()
        .gap(4.0)
        .child(ui::caption(
            format!("OpenCode · {} · {}", c.title(), c.machine().unwrap().state),
            k,
        ))
        .child(
            row()
                .gap(6.0)
                .child(
                    ui::Button::new("Changes")
                        .on_press(super::open_changes)
                        .build(k),
                )
                .child(
                    ui::Button::new("Refresh checkout")
                        .on_press(move |s: &mut Workspace| live_mut(s).app.refresh_remote(&a))
                        .build(k),
                )
                .child(
                    ui::Button::new("Stop machine")
                        .on_press(move |s: &mut Workspace| live_mut(s).app.power_remote(&b, false))
                        .build(k),
                ),
        )
}
