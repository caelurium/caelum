use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{Lifetime, Picker, Size, valid_branch};
use rune::{El, ScrollSpec, TextEdit, col, row};
use rune_kit::{Kit, ScrollBar};

pub(super) struct Form {
    base: TextEdit,
    branch: TextEdit,
    size: Size,
    lifetime: Lifetime,
}
impl Default for Form {
    fn default() -> Self {
        Self {
            base: TextEdit::default(),
            branch: TextEdit::default(),
            size: Size::Standard,
            lifetime: Lifetime::default(),
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
                            .on_press(move |s: &mut Workspace| {
                                live_mut(s).app.remote_picker(project.clone())
                            })
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
            &source.environment.as_ref().map_or_else(
                || "Create a dedicated Boat environment for this repository.".into(),
                |name| format!("Start from a published branch in the {name} environment."),
            ),
            k,
        ))
        .child(ui::caption("Base branch", k))
        .child(
            ui::field(&form.base, "Base branch", &source.base, k)
                .key("remote-base")
                .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.form.base),
        )
        .child(ui::caption("New branch on VM", k))
        .child(
            ui::field(&form.branch, "New branch", "Enter a branch name", k)
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
    let mut ttl = row()
        .flex_wrap()
        .center_y()
        .gap(4.0)
        .child(ui::caption("Auto-stop", k));
    for lifetime in Lifetime::CHOICES {
        ttl = ttl.child(
            ui::tab(lifetime.label(), form.lifetime == lifetime, k)
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.lifetime = lifetime),
        );
    }
    let hint = if form.branch.text.is_empty() {
        Some("Enter a new branch name, such as feat/my-change.")
    } else if !valid_branch(base) {
        Some("Choose a valid published base branch.")
    } else if !valid_branch(&form.branch.text) || form.branch.text == *base {
        Some("Choose a valid new branch name different from the base.")
    } else {
        None
    };
    let base = base.clone();
    body = body
        .child(sizes)
        .child(ttl)
        .children((form.lifetime == Lifetime::UntilStopped).then(|| {
            ui::caption(
                "Runs and bills until you stop the VM. Requires a paid Boat plan.",
                k,
            )
            .wrap()
        }))
        .child(
            row()
                .w_full()
                .gap(10.0)
                .center_y()
                .child(
                    ui::Button::new("Create VM")
                        .variant(rune_kit::ButtonVariant::Contrast)
                        .disabled(hint.is_some())
                        .on_press(move |s: &mut Workspace| {
                            let l = live_mut(s);
                            let f = &l.remote.form;
                            if l.app
                                .create_remote(&base, &f.branch.text, f.size, f.lifetime)
                                .is_some()
                            {
                                super::checkout::enter(s);
                            }
                        })
                        .build(k)
                        .key("create-remote"),
                )
                .children(hint.map(|text| ui::caption(text, k).grow(1.0).min_w(0.0).wrap())),
        );
    if l.panels.overlay != Some(Screen::CreateVm)
        && let Some(inventory) = &l.app.remote.inventory
    {
        body = body.child(ui::caption("Existing machines", k));
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
        .child(body)
        .scroll_bar(k)
}
