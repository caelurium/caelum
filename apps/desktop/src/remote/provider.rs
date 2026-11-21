mod effort;
mod model;

use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    select::{Choice, Select},
};
use caelum_core::{application::Provider, remote::Thread};
use rune::{El, TextEdit, row};
use rune_kit::{Kit, Placement};

#[derive(Clone, PartialEq, Eq)]
struct Owner {
    checkout: String,
    thread: String,
    provider: Provider,
    model: Option<String>,
}

impl Owner {
    fn active(&self, l: &Live) -> bool {
        l.panels.screen == Screen::Remote
            && l.panels.overlay().is_none()
            && l.app.remote.saved.selected.as_ref() == Some(&self.checkout)
            && l.app.can_switch_vm_thread(&self.checkout)
            && l.app
                .remote
                .saved
                .checkouts
                .get(&self.checkout)
                .is_some_and(|c| {
                    c.thread.editor_key(&self.checkout) == self.thread
                        && c.thread.provider == self.provider
                        && c.thread.model == self.model
                })
    }
}

pub(super) struct Menu {
    owner: Owner,
    kind: Kind,
}

enum Kind {
    Provider,
    Model(Box<TextEdit>),
    Effort,
}

pub(super) fn sync(l: &mut Live) -> bool {
    if l.remote.menu.as_ref().is_some_and(|m| !m.owner.active(l)) {
        l.remote.menu = None;
        return true;
    }
    false
}

fn close(s: &mut Workspace) {
    live_mut(s).remote.menu = None;
}

pub(super) fn controls(
    s: &Workspace,
    k: &Kit,
    id: &str,
    thread: &Thread,
    layers: &mut Vec<El>,
) -> El {
    let l = live(s);
    let owner = Owner {
        checkout: id.into(),
        thread: thread.editor_key(id).into(),
        provider: thread.provider,
        model: thread.model.clone(),
    };
    let open = matches!(&l.remote.menu, Some(Menu {owner: o, kind: Kind::Provider}) if o == &owner);
    let toggle = owner.clone();
    let choose = owner.clone();
    let provider = Select::choices(
        "vm-provider",
        Provider::ALL
            .into_iter()
            .map(|p| Choice::new(p.as_str(), p.name()))
            .collect(),
    )
    .label("VM provider")
    .menu_description("Applies to the next message in this chat.")
    .current(Some(thread.provider.as_str()))
    .width(248.0)
    .placement(Placement::TopStart)
    .disabled(!owner.active(l))
    .open(open)
    .on_toggle(move |s: &mut Workspace| {
        let l = live_mut(s);
        if toggle.active(l) {
            l.remote.menu = (!open).then(|| Menu {
                owner: toggle.clone(),
                kind: Kind::Provider,
            });
        }
    })
    .on_close(close)
    .on_choose(move |s: &mut Workspace, value, _| {
        let l = live_mut(s);
        if matches!(&l.remote.menu, Some(Menu {owner, kind: Kind::Provider}) if owner == &choose)
            && choose.active(l)
            && let Some(p) = Provider::ALL.into_iter().find(|p| p.as_str() == value)
        {
            l.app.set_vm_provider(&choose.checkout, &choose.thread, p);
            l.remote.menu = None;
            super::conversation::focus(l);
        }
    })
    .build(k, layers);
    row()
        .min_w(0.0)
        .flex_wrap()
        .center_y()
        .gap(2.0)
        .child(provider)
        .child(model::choice(s, k, &owner, layers))
        .children(effort::choice(
            s,
            k,
            &owner,
            thread.effort.as_deref(),
            layers,
        ))
}
