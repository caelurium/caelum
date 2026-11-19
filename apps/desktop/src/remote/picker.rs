use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::{
    application::VmDestination,
    remote::{Lifetime, Picker, Size, Source, valid_branch},
};
use rune::{El, ScrollSpec, TextEdit, col, row};
use rune_kit::{Kit, ScrollBar};

pub(super) struct Form {
    base: TextEdit,
    branch: TextEdit,
    size: Size,
    lifetime: Lifetime,
    tab: Tab,
}

#[derive(PartialEq, Eq)]
enum Tab {
    Existing,
    New,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            base: TextEdit::default(),
            branch: TextEdit::default(),
            size: Size::Standard,
            lifetime: Lifetime::default(),
            tab: Tab::Existing,
        }
    }
}

pub(crate) fn open(s: &mut Workspace, project: String) {
    let l = live_mut(s);
    l.app.dismiss_vm_handoff();
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
    let handoff = l.panels.overlay == Some(Screen::VmHandoff);
    if handoff && has_machines(s, source) {
        let existing = l.remote.form.tab == Tab::Existing;
        body = body.child(
            row()
                .gap(6.0)
                .child(
                    ui::tab("Existing VM", existing, k)
                        .key("handoff-existing-vm")
                        .on_press(|s: &mut Workspace| live_mut(s).remote.form.tab = Tab::Existing),
                )
                .child(
                    ui::tab("New VM", !existing, k)
                        .key("handoff-new-vm")
                        .on_press(|s: &mut Workspace| live_mut(s).remote.form.tab = Tab::New),
                ),
        );
        if existing {
            return scroll(
                body.child(ui::section(
                    &source.repository,
                    &format!("Choose a checkout containing origin/{}.", source.base),
                    k,
                ))
                .child(machines(s, k, source, true)),
                k,
            );
        }
    }
    let form = &l.remote.form;
    let base = if handoff || form.base.text.is_empty() {
        &source.base
    } else {
        &form.base.text
    };
    body = body.child(ui::section(
        &source.repository,
        &source.environment.as_ref().map_or_else(
            || "Create a Boat environment for this repository (needs environment.write).".into(),
            |name| format!("Use the {name} environment on Boat."),
        ),
        k,
    ));
    let hint = if handoff {
        body = body.child(
            ui::caption(format!("Session branch · {}", source.base), k)
                .key("handoff-base")
                .wrap(),
        );
        (source.published_tip.is_none() || !valid_branch(base))
            .then_some("The session branch must be published to origin.")
    } else {
        body = body
            .child(
                col()
                    .w_full()
                    .gap(4.0)
                    .child(ui::caption("Base branch", k))
                    .child(
                        ui::field(&form.base, "Base branch", &source.base, k)
                            .key("remote-base")
                            .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.form.base),
                    ),
            )
            .child(ui::caption("New branch on VM", k))
            .child(
                ui::field(&form.branch, "New branch", "Enter a branch name", k)
                    .key("remote-branch")
                    .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.form.branch),
            );
        if form.branch.text.is_empty() {
            Some("Enter a new branch name, such as feat/my-change.")
        } else if !valid_branch(base) {
            Some("Choose a valid published base branch.")
        } else if !valid_branch(&form.branch.text) || form.branch.text == *base {
            Some("Choose a valid new branch name different from the base.")
        } else {
            None
        }
    };
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
        .w_full()
        .flex_wrap()
        .center_y()
        .gap(4.0)
        .child(ui::caption("Auto-stop", k));
    for lifetime in Lifetime::CHOICES {
        ttl = ttl.child(
            ui::tab(lifetime.label(), form.lifetime == lifetime, k)
                .key(format!("vm-lifetime-{}", lifetime.label()))
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.lifetime = lifetime),
        );
    }
    let base = base.clone();
    let note = match form.lifetime {
        Lifetime::UntilStopped => {
            "Runs and bills until you stop the VM. Requires a paid Boat plan."
        }
        Lifetime::Day | Lifetime::EightHours => {
            "Paid plan. Active or waiting OpenCode prompts renew the timer; standalone commands don’t."
        }
        _ => "Active or waiting OpenCode prompts renew the timer; standalone commands don’t.",
    };
    body = body
        .child(sizes)
        .child(ttl)
        .child(ui::caption(note, k).wrap())
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
                            if handoff {
                                l.app.confirm_vm_handoff(VmDestination::Create {
                                    size: f.size,
                                    lifetime: f.lifetime,
                                });
                                l.schedule();
                            } else if l
                                .app
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
    if l.panels.overlay == Some(Screen::RemotePicker) {
        body = body.child(machines(s, k, source, false));
    }
    scroll(body, k)
}

fn scroll(body: El, k: &Kit) -> El {
    col()
        .h_full()
        .w_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
        .scroll_bar(k)
}

fn has_machines(s: &Workspace, source: &Source) -> bool {
    live(s)
        .app
        .remote
        .inventory
        .as_ref()
        .is_some_and(|i| i.machines.iter().any(|m| m.key.account == source.account))
}

pub(crate) fn handoff_height(s: &Workspace) -> f32 {
    let l = live(s);
    if l.app.vm_handoff().is_none() || l.app.remote.inventory.is_none() {
        return 256.0;
    }
    if let Some(Picker::Ready(source)) = &l.app.remote.picker
        && has_machines(s, source)
        && l.remote.form.tab == Tab::Existing
    {
        return 280.0;
    }
    400.0
}

fn machines(s: &Workspace, k: &Kit, source: &Source, handoff: bool) -> El {
    let app = &live(s).app;
    let mut list = col().w_full().gap(4.0);
    let Some(inventory) = &app.remote.inventory else {
        return list;
    };
    let mut machines = inventory
        .machines
        .iter()
        .filter(|m| m.key.account == source.account)
        .peekable();
    if !handoff && machines.peek().is_some() {
        list = list.child(ui::caption("Use an existing VM", k));
    }
    for m in machines {
        let machine = m.clone();
        let blocker = handoff.then(|| app.handoff_machine_blocker(m)).flatten();
        list = list.child(
            ui::Button::new(format!("{} · {}", m.name, blocker.unwrap_or(&m.state)))
                .disabled(blocker.is_some())
                .on_press(move |s: &mut Workspace| {
                    let app = &mut live_mut(s).app;
                    if handoff {
                        app.confirm_vm_handoff(VmDestination::Existing(machine.clone()));
                        live_mut(s).schedule();
                    } else if app.attach_remote(machine.clone()).is_some() {
                        super::checkout::enter(s);
                    }
                })
                .build(k)
                .key(format!("attach-vm-{}", m.key.id)),
        );
    }
    list
}
