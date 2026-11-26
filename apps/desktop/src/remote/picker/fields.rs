use super::{Lifetime, Size, Source};
use crate::{
    Workspace,
    live::{live, live_mut},
    ui,
};
use rune::{Cursor, Drag, El, Phase, Tx, col, el, icon, row, text};
use rune_kit::Kit;

fn label(value: &str, k: &Kit) -> El {
    text(value).font(k.ty.small_medium).color(k.c.text_base)
}

pub(super) fn checkout(k: &Kit, source: &Source) -> El {
    if source.published_tip.is_none() {
        return ui::caption("Could not verify a published branch or origin/main. Check Git access and reopen VM setup.", k).wrap();
    }
    if let Some(publication) = &source.publish {
        let destination = match publication {
            caelum_core::remote::BranchPublication::Named(name) => format!("origin/{name}"),
            _ => "a new origin/caelum/<session-id> branch".into(),
        };
        return ui::caption(
            format!("Create {destination} from origin/main, then start the VM."),
            k,
        )
        .wrap()
        .key("handoff-base");
    }
    let tip = source.published_tip.as_deref().unwrap_or("");
    row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(ui::caption("Session branch", k))
        .child(
            row()
                .center_y()
                .gap(5.0)
                .px(7.0)
                .py(2.0)
                .radius(5.0)
                .fill(k.c.layer_02)
                .min_w(0.0)
                .shrink()
                .child(icon(&ui::BRANCH, 12.0).color(k.c.icon_muted).a11y_hidden())
                .child(
                    text(&source.base)
                        .font(k.ty.small_medium)
                        .color(k.c.text_base)
                        .min_w(0.0)
                        .truncate()
                        .help(format!("Published session branch: {}", source.base))
                        .key("handoff-base"),
                ),
        )
        .children(tip.get(..8).map(|short| ui::caption(short, k).help(tip)))
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
    let size_index = SIZES
        .iter()
        .position(|(size, _, _)| *size == form.size)
        .unwrap_or(1);
    let (_, size_name, size_spec) = SIZES[size_index];
    let size_stops: Vec<Stop> = SIZES
        .iter()
        .map(|(_, name, spec)| Stop {
            key: format!("vm-size-{name}"),
            label: name,
            description: Some(spec),
        })
        .collect();
    let size_slider = stepped_slider(
        "Machine size",
        &format!("{size_name} · {size_spec}"),
        "vm-size-slider",
        &size_stops,
        size_index,
        |s, idx| {
            if let Some(&(size, _, _)) = SIZES.get(idx) {
                live_mut(s).remote.form.size = size;
            }
        },
        k,
    );

    let lifetime_index = Lifetime::CHOICES
        .iter()
        .position(|&value| value == form.lifetime)
        .unwrap_or(1);
    let lifetime_stops: Vec<Stop> = Lifetime::CHOICES
        .iter()
        .map(|value| Stop {
            key: format!("vm-lifetime-{}", value.label()),
            label: value.label(),
            description: None,
        })
        .collect();
    let lifetime_slider = stepped_slider(
        "Auto-stop",
        form.lifetime.label(),
        "vm-lifetime-slider",
        &lifetime_stops,
        lifetime_index,
        |s, idx| {
            if let Some(&lifetime) = Lifetime::CHOICES.get(idx) {
                live_mut(s).remote.form.lifetime = lifetime;
            }
        },
        k,
    );

    let detail = match form.lifetime {
        Lifetime::UntilStopped => "Bills until you stop the VM. Requires a paid Boat plan.",
        Lifetime::Day | Lifetime::EightHours => {
            "Paid plans only. SSH sessions do not extend the timer."
        }
        _ => "SSH sessions do not extend the auto-stop timer.",
    };
    col().w_full().gap(14.0).child(size_slider).child(
        col()
            .w_full()
            .gap(4.0)
            .child(lifetime_slider)
            .child(ui::caption(detail, k).wrap().key("vm-lifetime-detail")),
    )
}

struct Stop {
    key: String,
    label: &'static str,
    description: Option<&'static str>,
}

fn stepped_slider(
    title: &'static str,
    readout: &str,
    track_key: &'static str,
    stops: &[Stop],
    selected: usize,
    on_select: impl Fn(&mut Workspace, usize) + Copy + 'static,
    k: &Kit,
) -> El {
    let count = stops.len();
    let max_index = count.saturating_sub(1).max(1);
    let progress = selected as f32 / max_index as f32;
    let track_width = rune::memo::window_layout(track_key).map_or(320.0, |r| r[2].max(64.0));
    let thumb = 16.0;
    let inset = thumb / 2.0;
    let usable = (track_width - thumb).max(1.0);
    let thumb_x = progress * usable;

    let mut track = el()
        .key(track_key)
        .w_full()
        .h(24.0)
        .cursor(Cursor::Pointer)
        .child(
            el().absolute(inset, 10.0)
                .w(usable)
                .h(4.0)
                .radius(2.0)
                .fill(k.c.layer_02)
                .a11y_hidden(),
        )
        .child(
            el().absolute(inset, 10.0)
                .w(thumb_x.max(0.0))
                .h(4.0)
                .radius(2.0)
                .fill(k.c.text_accent)
                .a11y_hidden(),
        );
    for i in 0..count {
        let x = inset + (i as f32 / max_index as f32) * usable - 2.5;
        track = track.child(
            el().absolute(x, 9.5)
                .size(5.0, 5.0)
                .radius(2.5)
                .fill(if i <= selected {
                    k.c.text_accent
                } else {
                    k.c.border_strong
                })
                .a11y_hidden(),
        );
    }
    track = track
        .child(
            el().absolute(thumb_x, 4.0)
                .size(thumb, thumb)
                .radius(thumb / 2.0)
                .fill(k.c.layer_01)
                .border(2.0, k.c.text_accent)
                .shadow(0.0, 2.0, k.alpha(rune::Color::BLACK, 0.25))
                .a11y_hidden(),
        )
        .on_drag(move |s: &mut Workspace, d: &mut Drag, tx: &mut Tx| {
            let step_w = usable / max_index as f32;
            match d.phase {
                Phase::Down => d.data = selected as f32,
                Phase::Move => {
                    tx.instant();
                    let next = (d.data + d.dx / step_w)
                        .round()
                        .clamp(0.0, max_index as f32) as usize;
                    on_select(s, next);
                }
                Phase::Up => {
                    if d.cancelled() {
                        on_select(s, d.data.round().clamp(0.0, max_index as f32) as usize);
                    }
                }
            }
        });

    let mut labels = row()
        .w_full()
        .justify_between()
        .center_y()
        .role("radiogroup")
        .label(title);
    for (i, stop) in stops.iter().enumerate() {
        let is_selected = i == selected;
        let mut item = k
            .interactive(
                row().px(4.0).py(2.0).radius(4.0).center().child(
                    text(stop.label)
                        .font(if is_selected {
                            k.ty.small_medium
                        } else {
                            k.ty.small
                        })
                        .color(if is_selected {
                            k.c.text_base
                        } else {
                            k.c.text_muted
                        }),
                ),
            )
            .role("radio")
            .label(stop.label)
            .checked(is_selected)
            .focusable(true)
            .focus_selected(is_selected)
            .focus_ring(1.5, 1.0, k.c.border_focus)
            .key(&stop.key)
            .on_press(move |s: &mut Workspace| on_select(s, i));
        if let Some(description) = stop.description {
            item = item.description(description);
        }
        labels = labels.child(item);
    }

    col()
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(label(title, k))
                .child(el().grow(1.0))
                .child(text(readout).font(k.ty.small_medium).color(k.c.text_accent)),
        )
        .child(track)
        .child(labels)
}
