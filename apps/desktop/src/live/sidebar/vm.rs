use super::{live, row::Card};
use crate::{Workspace, panels::Screen, ui};
use caelum_core::remote::{Turn, VmLink};
use rune::{El, col, icon, row, text};
use rune_kit::Kit;

pub(super) fn view(s: &Workspace, k: &Kit, link: &VmLink, position: usize, count: usize) -> El {
    let l = live(s);
    let app = &l.app;
    let Some((c, t)) = link.resolve(&app.remote.saved) else {
        return col();
    };
    let current = c.thread.editor_key(&link.checkout) == link.thread;
    let selected = current
        && l.panels.screen == Screen::Remote
        && app.remote.saved.selected.as_ref() == Some(&link.checkout);
    let available = app.can_open_vm_thread(link);
    let title = t.title();
    let project = crate::checkouts::project_name(app, app.group_path(&c.source.project));
    let provider = ui::provider_name(t.provider);
    let age = super::age(t.last_activity, l.feed.displayed_at()).0;
    let destination = c
        .branch()
        .map_or_else(|| c.title().to_owned(), |b| format!("{} · {b}", c.title()));
    let bottom = row()
        .w_full()
        .h(20.)
        .gap(4.)
        .center_y()
        .child(icon(&ui::SERVER, 11.).color(k.c.icon_muted))
        .child(
            text(&destination)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate()
                .min_w(0.)
                .grow(1.),
        );
    let card = Card {
        badge: l.feed.badges.view(app, &c.source.project, k),
        project,
        title,
        provider,
        age: &age,
        ink: k.c.text_muted,
        mark: (t.turn != Turn::Idle)
            .then(|| icon(&ui::CLOCK, 10.).color(k.c.icon_muted).a11y_hidden()),
    }
    .build(k, bottom, selected, position, count)
    .description(format!("{project}, VM {destination}, {provider}"))
    .help(if available {
        format!(
            "{title}\nVM {destination}\n{} · {provider}",
            c.source.repository
        )
    } else {
        format!("Finish the current VM operation to switch to {title}.")
    })
    .disabled(!available)
    .focusable(available);
    let target = link.clone();
    let card = card.on_press(move |s: &mut Workspace| crate::remote::open_session_row(s, &target));
    ui::selected_row(card, selected, k).mb(4.)
}
