use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{Picker, Size, valid_branch};
use rune::{El, ScrollSpec, TextEdit, col, row};
use rune_kit::{Kit, ScrollBar};

pub(super) struct Form {
    base: TextEdit,
    branch: TextEdit,
    size: Size,
    minutes: u32,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            base: TextEdit::default(),
            branch: TextEdit::default(),
            size: Size::Standard,
            minutes: 60,
        }
    }
}

pub(crate) fn open(s: &mut Workspace, project: String) {
    let l = live_mut(s);
    l.remote.form = Form::default();
    l.app.remote_picker(project);
    l.panels.overlay = Some(Screen::RemotePicker);
    l.schedule();
}

pub(crate) fn picker(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let mut body = col().w_full().gap(8.0);
    let source = match &l.app.remote.picker {
        Some(Picker::Ready(source)) => source,
        Some(Picker::Failed { project, error }) => {
            let project = project.clone();
            return body.child(ui::caption(error, k).wrap()).child(
                row()
                    .gap(8.0)
                    .child(
                        ui::Button::new("Connection settings")
                            .on_press(|s: &mut Workspace| {
                                let l = live_mut(s);
                                l.panels.settings_page = crate::settings::Page::Remote;
                                l.panels.overlay = Some(Screen::Settings);
                            })
                            .build(k),
                    )
                    .child(
                        ui::Button::new("Retry")
                            .on_press(move |s: &mut Workspace| open(s, project.clone()))
                            .build(k),
                    ),
            );
        }
        _ => return body.child(ui::caption("Looking up the project on Boat…", k)),
    };
    let form = &l.remote.form;
    let base = if form.base.text.is_empty() {
        &source.base
    } else {
        &form.base.text
    };
    body = body
        .child(ui::section(
            &source.repository,
            "Create a machine from a published base branch.",
            k,
        ))
        .child(ui::caption("Base branch", k))
        .child(
            ui::field(&form.base, "Base branch", &source.base, k)
                .key("remote-base")
                .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.form.base),
        )
        .child(ui::caption("New branch on machine", k))
        .child(
            ui::field(&form.branch, "New branch", "feat/my-change", k)
                .key("remote-branch")
                .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.form.branch),
        );
    let mut sizes = row().center_y().gap(4.0).child(ui::caption("Size", k));
    for (name, size) in [
        ("Small", Size::Small),
        ("Standard", Size::Standard),
        ("Large", Size::Large),
        ("XL", Size::Xlarge),
    ] {
        sizes = sizes.child(
            ui::tab(name, form.size == size, k)
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.size = size),
        );
    }
    let mut ttl = row().center_y().gap(4.0).child(ui::caption("Auto-stop", k));
    for minutes in [30, 60, 120] {
        ttl = ttl.child(
            ui::tab(&format!("{minutes} min"), form.minutes == minutes, k)
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.minutes = minutes),
        );
    }
    let base = base.clone();
    body = body
        .child(sizes)
        .child(ttl)
        .child(
            ui::Button::new("Create and open")
                .disabled(
                    !valid_branch(&form.branch.text)
                        || !valid_branch(&base)
                        || form.branch.text == base,
                )
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    let f = &l.remote.form;
                    if l.app
                        .create_remote(&base, &f.branch.text, f.size, f.minutes)
                        .is_some()
                    {
                        super::checkout::enter(s);
                    }
                })
                .build(k)
                .key("create-remote"),
        )
        .child(ui::caption("Existing machines", k));
    if let Some(inventory) = &l.app.remote.inventory {
        for m in inventory
            .machines
            .iter()
            .filter(|m| m.key.account == source.account)
        {
            let machine = m.clone();
            body = body.child(
                ui::Button::new(format!("{} · {}", m.name, m.state))
                    .on_press(move |s: &mut Workspace| {
                        if live_mut(s).app.attach_remote(machine.clone()).is_some() {
                            super::checkout::enter(s);
                        }
                    })
                    .build(k),
            );
        }
    }
    col()
        .h_full()
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .scroll_bar(k)
        .child(body)
}
