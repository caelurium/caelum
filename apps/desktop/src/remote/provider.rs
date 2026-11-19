use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    select::{Choice, Select},
};
use caelum_core::{application::Provider, remote::Thread};
use rune::El;
use rune_kit::{Kit, Placement};

pub(super) fn sync(l: &mut Live) -> bool {
    let Some(owner) = &l.remote.provider_menu else {
        return false;
    };
    if l.panels.screen == Screen::Remote
        && l.panels.overlay.is_none()
        && l.app.remote.saved.selected.as_ref().is_some_and(|id| {
            l.app.can_switch_vm_thread(id)
                && l.app.remote.saved.checkouts[id].thread.editor_key(id) == owner
        })
    {
        return false;
    }
    l.remote.provider_menu = None;
    true
}

pub(super) fn choice(
    s: &Workspace,
    k: &Kit,
    id: &str,
    thread: &Thread,
    layers: &mut Vec<El>,
) -> El {
    let l = live(s);
    let owner = thread.editor_key(id).to_owned();
    let toggle = owner.clone();
    let checkout = id.to_owned();
    Select::choices(
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
    .disabled(!l.app.can_switch_vm_thread(id))
    .open(l.remote.provider_menu.as_deref() == Some(&owner))
    .on_toggle(move |s: &mut Workspace| {
        let menu = &mut live_mut(s).remote.provider_menu;
        *menu = (menu.as_ref() != Some(&toggle)).then(|| toggle.clone());
    })
    .on_close(|s: &mut Workspace| live_mut(s).remote.provider_menu = None)
    .on_choose(move |s: &mut Workspace, value, _| {
        let l = live_mut(s);
        if l.remote.provider_menu.take().as_deref() == Some(&owner)
            && let Some(p) = Provider::ALL.into_iter().find(|p| p.as_str() == value)
        {
            l.app.set_vm_provider(&checkout, &owner, p);
            super::conversation::focus(l);
        }
    })
    .build(k, layers)
}
