mod changes;
mod checkout;
mod conversation;
mod picker;

pub(crate) use changes::{inspector, open as open_changes};
pub(crate) use checkout::{screen, select, sync};
pub(crate) use picker::{open, picker};

use crate::{
    Workspace,
    live::{live, live_mut},
    settings::heading,
    ui,
};
use caelum_core::remote::Connection;
use rune::{El, TextEdit, col, row};
use rune_kit::Kit;

#[derive(Default)]
pub(crate) struct State {
    key: TextEdit,
    form: picker::Form,
    restored: bool,
    editors: std::collections::HashMap<String, conversation::Editors>,
    details: bool,
    review: changes::Panel,
}

pub(crate) fn settings(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let remote = &l.app.remote;
    let busy = matches!(remote.connection, Connection::Loading(_));
    let mut view = col()
        .w_full()
        .gap(8.0)
        .child(heading(
            "Remote machines",
            "Connect Boat to use your remote machines.",
            k,
        ))
        .child(
            ui::field(&l.remote.key, "Boat API key", "Boat API key", k)
                .secure()
                .key("boat-api-key")
                .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.key),
        )
        .child(
            row()
                .gap(8.0)
                .child(
                    ui::Button::new("Connect Boat")
                        .disabled(busy || l.remote.key.text.trim().is_empty())
                        .on_press(|s: &mut Workspace| {
                            let l = live_mut(s);
                            let mut editor = std::mem::take(&mut l.remote.key);
                            l.app.connect_boat(std::mem::take(&mut editor.text));
                            l.schedule();
                        })
                        .build(k),
                )
                .child(
                    ui::Button::new("Refresh")
                        .disabled(busy)
                        .on_press(|s: &mut Workspace| {
                            let l = live_mut(s);
                            l.app.list_machines();
                            l.schedule();
                        })
                        .build(k),
                )
                .children(remote.inventory.as_ref().map(|_| {
                    ui::Button::new("Disconnect")
                        .disabled(busy)
                        .on_press(|s: &mut Workspace| {
                            let l = live_mut(s);
                            l.app.disconnect_boat();
                            l.schedule();
                        })
                        .build(k)
                })),
        );
    if busy {
        view = view.child(ui::caption("Connecting to Boat…", k));
    }
    if let Connection::Failed(error) = &remote.connection {
        view = view.child(ui::caption(error, k).wrap().color(k.c.danger.fg));
    }
    if let Some(list) = &remote.inventory {
        if list.machines.is_empty() {
            view = view.child(ui::caption("Connected · No machines yet", k));
        }
        for machine in &list.machines {
            view = view.child(ui::caption(
                format!("{} · {}", machine.name, machine.state),
                k,
            ));
        }
    }
    view
}
