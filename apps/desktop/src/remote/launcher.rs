use crate::{Workspace, live::live, ui};
use caelum_core::remote::Phase;
use rune::{El, ScrollSpec, col};
use rune_kit::{Kit, ScrollBar};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

pub(crate) fn group(
    s: &Workspace,
    k: &Kit,
    project: &str,
    query: &str,
    matches_project: bool,
) -> Option<El> {
    let app = &live(s).app;
    let ids: Vec<_> = app
        .remote
        .saved
        .checkouts
        .iter()
        .filter_map(|(id, c)| {
            let Phase::Ready { machine, branch } = &c.phase else {
                return None;
            };
            (app.group_path(&c.source.project) == project
                && (matches_project
                    || format!("{} {branch} {}", machine.name, c.source.repository)
                        .to_lowercase()
                        .contains(query)))
            .then(|| id.clone())
        })
        .collect();
    if ids.is_empty() {
        return None;
    }
    let mut version = DefaultHasher::new();
    ids.hash(&mut version);
    let ids = Rc::new(ids);
    let keys = Rc::clone(&ids);
    let kit = k.clone();
    let height = ids.len().min(3) as f32 * 48.0;
    let rows = col()
        .w_full()
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            ids.len(),
            48.0,
            move |_: &Workspace, i| format!("launch-vm-{}", keys[i]).into(),
            move |s: &Workspace, i| {
                let app = &live(s).app;
                let id = &ids[i];
                let Some(Phase::Ready { machine, branch }) =
                    app.remote.saved.checkouts.get(id).map(|c| &c.phase)
                else {
                    return col();
                };
                let ready = !matches!(
                    app.remote.jobs.get(id),
                    Some(caelum_core::remote::Job::Deleting(_))
                );
                let owner = id.clone();
                ui::choice(&machine.name, branch, false, &kit)
                    .disabled(!ready)
                    .focusable(ready)
                    .label(format!("New session on VM {}", machine.name))
                    .on_press(move |s: &mut Workspace| super::new_session(s, &owner))
            },
        )
        .virtual_rows_version(version.finish());
    Some(
        col()
            .w_full()
            .pl(24.0)
            .gap(2.0)
            .child(ui::caption("Remote VMs", k).px(10.0).py(3.0))
            .child(
                col()
                    .w_full()
                    .h(height)
                    .key(crate::code_view::source_key(project, "launch-vms"))
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(rows)
                    .scroll_bar(k),
            ),
    )
}
