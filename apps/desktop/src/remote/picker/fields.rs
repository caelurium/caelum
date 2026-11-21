use super::{Lifetime, Size, Source, submit, valid_branch};
use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    ui,
};
use rune::{El, Key, col, icon, row, text};
use rune_kit::Kit;

fn label(value: &str, k: &Kit) -> El {
    text(value).font(k.ty.small_medium).color(k.c.text_base)
}

pub(super) fn checkout(s: &Workspace, k: &Kit, source: &Source, handoff: bool) -> El {
    let l = live(s);
    let f = &l.remote.form;
    if handoff {
        let tip = source.published_tip.as_deref().unwrap_or("");
        return row()
            .w_full()
            .center_y()
            .gap(6.0)
            .child(icon(&ui::BRANCH, 13.0).color(k.c.icon_muted))
            .child(ui::caption("Session branch", k))
            .child(
                text(&source.base)
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .min_w(0.0)
                    .truncate()
                    .help(format!("Published session branch: {}", source.base))
                    .key("handoff-base"),
            )
            .children(
                tip.get(..8)
                    .map(|short| ui::caption(format!("· {short}"), k).help(tip)),
            );
    }
    let owner = source.clone();
    let screen = l.panels.overlay;
    let base = col()
        .min_w(0.0)
        .grow(1.0)
        .gap(4.0)
        .child(label("Start from", k))
        .child(
            ui::field(&f.base, "Published base branch", &source.base, k)
                .edit_menu("remote-base", |s| Some(&mut live_mut(s).remote.form.base)),
        )
        .children((!valid_branch(f.base(source))).then(|| {
            ui::caption("Enter a valid published branch.", k)
                .wrap()
                .color(k.c.danger.fg)
                .role("alert")
        }));
    let branch = col()
        .min_w(0.0)
        .grow(1.0)
        .gap(4.0)
        .child(label("New branch on VM", k))
        .child(
            ui::field(&f.branch, "New branch", "feat/my-change", k)
                .edit_menu("remote-branch", |s| {
                    Some(&mut live_mut(s).remote.form.branch)
                })
                .on_input_key(move |s: &mut Workspace, e, _| {
                    if e.key != Key::Enter {
                        return false;
                    }
                    submit(s, &owner, screen);
                    true
                }),
        )
        .children(f.branch_error(source).map(|e| {
            ui::caption(e, k)
                .wrap()
                .color(k.c.danger.fg)
                .role("alert")
                .key("vm-branch-error")
        }));
    let pair = if rune::memo::window_layout("shell").is_none_or(|r| r[2] >= 520.) {
        row()
    } else {
        col()
    };
    pair.w_full().gap(10.0).child(base).child(branch)
}

// Boat's standard size names and capacities: https://docs.boat.dev/machines.md
const SIZES: [(Size, &str, &str); 4] = [
    (Size::Small, "Small", "2 vCPU · 4 GB"),
    (Size::Standard, "Standard", "4 vCPU · 8 GB"),
    (Size::Large, "Large", "8 vCPU · 16 GB"),
    (Size::Xlarge, "XL", "16 vCPU · 32 GB"),
];

pub(super) fn machine(s: &Workspace, k: &Kit) -> El {
    let form = &live(s).remote.form;
    let mut sizes = row()
        .w_full()
        .flex_wrap()
        .gap(4.0)
        .role("radiogroup")
        .label("Machine size");
    for (size, name, spec) in SIZES {
        let selected = form.size == size;
        let card = col()
            .grow(1.0)
            .min_w(84.0)
            .center()
            .min_h(46.0)
            .px(4.0)
            .gap(2.0)
            .child(text(name).font(k.ty.ui_medium).color(if selected {
                k.c.text_accent
            } else {
                k.c.text_base
            }))
            .child(ui::caption(spec, k));
        sizes = sizes.child(
            option(card, name, selected, k)
                .description(spec)
                .key(format!("vm-size-{name}"))
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.size = size),
        );
    }
    let mut timers = row()
        .w_full()
        .flex_wrap()
        .gap(4.0)
        .role("radiogroup")
        .label("Auto-stop");
    for value in Lifetime::CHOICES {
        let name = value.label();
        let selected = form.lifetime == value;
        let chip =
            row()
                .min_h(28.0)
                .px(8.0)
                .center()
                .child(text(name).font(k.ty.small_medium).color(if selected {
                    k.c.text_accent
                } else {
                    k.c.text_muted
                }));
        timers = timers.child(
            option(chip, name, selected, k)
                .key(format!("vm-lifetime-{name}"))
                .on_press(move |s: &mut Workspace| live_mut(s).remote.form.lifetime = value),
        );
    }
    let detail = match form.lifetime {
        Lifetime::UntilStopped => "Bills until you stop the VM. Requires a paid Boat plan.",
        Lifetime::Day | Lifetime::EightHours => {
            "Paid plans only. Boat renews the timer during a prompt."
        }
        _ => "Boat renews the timer while a prompt is running or waiting.",
    };
    col()
        .w_full()
        .gap(12.0)
        .child(
            col()
                .w_full()
                .gap(5.0)
                .child(label("Machine size", k))
                .child(sizes),
        )
        .child(
            col()
                .w_full()
                .gap(5.0)
                .child(label("Auto-stop", k))
                .child(timers)
                .child(ui::caption(detail, k).wrap().key("vm-lifetime-detail")),
        )
}

fn option(el: El, name: &str, selected: bool, k: &Kit) -> El {
    k.interactive(
        el.radius(6.0)
            .border(
                1.0,
                if selected {
                    k.c.border_focus
                } else {
                    k.c.border_base
                },
            )
            .fill(if selected {
                k.alpha(k.c.bg_accent, 0.09)
            } else {
                k.c.layer_01
            }),
    )
    .role("radio")
    .label(name)
    .checked(selected)
    .focusable(true)
    .focus_selected(selected)
    .focus_ring(1.5, 1.0, k.c.border_focus)
}
