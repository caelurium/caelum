use super::LAPTOP;
use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::{
    application::Task,
    remote::{Activity, Checkout},
};
use rune::{Color, El, TextStyle, col, el, icon, row, text};
use rune_kit::Kit;

pub(super) fn view(s: &Workspace, pane_width: f32, compact: bool, k: &Kit) -> El {
    let height = if compact { 118.0 } else { 148.0 };
    let activity = section_card("Sessions", activity_card(s, compact, k), height, k);
    let machines = section_card("Machines", machines_card(s, compact, k), height, k);
    if pane_width >= 520.0 {
        row()
            .key("overview-cards")
            .w_full()
            .gap(if compact { 8.0 } else { 12.0 })
            .child(activity.grow(1.0).min_w(0.0))
            .child(machines.grow(1.0).min_w(0.0))
    } else {
        col()
            .key("overview-cards")
            .w_full()
            .gap(8.0)
            .child(activity)
            .child(machines)
    }
}

fn section_card(title: &str, body: El, height: f32, k: &Kit) -> El {
    col()
        .w_full()
        .gap(6.0)
        .child(
            text(title)
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .px(2.0),
        )
        .child(
            body.w_full()
                .h(height)
                .padding(12.0)
                .radius(12.0)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_base),
        )
}

fn activity_card(s: &Workspace, compact: bool, k: &Kit) -> El {
    let l = live(s);
    let hist = l.app.activity_histogram(l.feed.displayed_at());
    let peak = hist.days.iter().copied().max().unwrap_or(1).max(1) as f32;
    let max_bar = if compact { 50.0 } else { 72.0 };
    let mut bars = row()
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .gap(if compact { 3.0 } else { 5.0 })
        .items_end();
    for (idx, count) in hist.days.iter().copied().enumerate() {
        let days_ago = 13 - idx;
        let when = match days_ago {
            0 => "Today".to_owned(),
            1 => "Yesterday".to_owned(),
            n => format!("{n} days ago"),
        };
        let label = format!(
            "{when} · {count} {}",
            if count == 1 { "session" } else { "sessions" }
        );
        let h = if count == 0 {
            4.0
        } else {
            (count as f32 / peak * max_bar).max(10.0)
        };
        let fill = if count == 0 {
            k.alpha(k.c.text_accent, 0.14)
        } else {
            k.alpha(k.c.text_accent, 0.85)
        };
        bars = bars.child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .h_full()
                .justify_end()
                .child(el().w_full().h(h).radius(3.0).fill(fill))
                .role("img")
                .label(&label)
                .help(label),
        );
    }
    let delta = hist.delta_percent.map(|pct| {
        let sign = if pct >= 0 { "+" } else { "" };
        let tone = if pct >= 0 {
            k.c.success.fg
        } else {
            k.c.danger.fg
        };
        text(format!("{sign}{pct}%"))
            .font(k.ty.small_medium)
            .color(tone)
    });
    col()
        .key("overview-card-activity")
        .gap(10.0)
        .child(
            row()
                .w_full()
                .items_end()
                .gap(7.0)
                .child(
                    text(hist.total.to_string())
                        .font(TextStyle::new(20.0, 600.0))
                        .color(k.c.text_base),
                )
                .child(ui::caption(
                    if compact {
                        "last 14 days"
                    } else {
                        "active in last 14 days"
                    },
                    k,
                ))
                .children(delta),
        )
        .child(bars)
}

fn machines_card(s: &Workspace, compact: bool, k: &Kit) -> El {
    let l = live(s);
    let mut rows = col()
        .key("overview-card-machines")
        .w_full()
        .h_full()
        .justify_center()
        .gap(if compact { 6.0 } else { 10.0 });
    if let Some(host) = &l.app.services.host {
        let mem = if compact {
            format!("{:.0}/{:.0}G", host.used_gb, host.total_gb)
        } else {
            format!("{:.0}/{:.0} GB", host.used_gb, host.total_gb)
        };
        let history: Vec<f32> = host.cpu_history.iter().copied().collect();
        let cpu = format!("{:.0}%", host.cpu_percent);
        rows = rows.child(
            machine_row(
                &LAPTOP,
                &host.name,
                &mem,
                (!compact).then_some(history.as_slice()),
                &cpu,
                k.c.text_muted,
                k,
            )
            .px(4.0),
        );
    } else {
        let name = if cfg!(target_os = "macos") {
            "Local Mac"
        } else {
            "Local machine"
        };
        rows = rows.child(
            machine_row(
                &LAPTOP,
                name,
                if l.app.services.busy(&Task::HostStats) {
                    "Sampling…"
                } else {
                    "Local"
                },
                None,
                "—",
                k.c.text_faint,
                k,
            )
            .px(4.0),
        );
    }
    let mut vms: Vec<(&String, &Checkout)> = l.app.remote.saved.checkouts.iter().collect();
    vms.sort_by(|a, b| a.1.title().cmp(b.1.title()));
    let shown = vms.len().min(2);
    for (id, checkout) in vms.into_iter().take(shown) {
        let sample = l.app.remote.monitor.sample(id);
        let state = checkout
            .machine()
            .map(|m| m.state.as_str())
            .unwrap_or("setup");
        let mem = sample
            .and_then(|sample| {
                let ram = sample.memory_gb?;
                let used = match &sample.activity {
                    Activity::Live(Some(p)) | Activity::LastSample(Some(p)) => p.memory_mb,
                    _ => None,
                };
                Some(match used {
                    Some(mb) => format!("{:.1}/{ram} GB", mb / 1024.0),
                    None => format!("{ram} GB"),
                })
            })
            .unwrap_or_else(|| state.to_owned());
        let cpu_val = sample.and_then(|sample| match &sample.activity {
            Activity::Live(Some(p)) | Activity::LastSample(Some(p)) => Some(p.cpu_percent as f32),
            _ => None,
        });
        let right = cpu_val
            .map(|pct| format!("{pct:.0}%"))
            .unwrap_or_else(|| state.to_owned());
        let spark = cpu_val.map(|v| vec![v * 0.6, v * 0.85, v, v * 0.9, v]);
        let tone = if state == "running" {
            k.c.text_muted
        } else {
            k.c.text_faint
        };
        let target = id.clone();
        rows = rows.child(
            k.interactive(
                machine_row(
                    &ui::SERVER,
                    checkout.title(),
                    &mem,
                    (!compact).then_some(spark.as_deref()).flatten(),
                    &right,
                    tone,
                    k,
                )
                .radius(6.0)
                .px(4.0)
                .py(2.0),
            )
            .role("button")
            .label(format!("Open VM {}", checkout.title()))
            .focusable(true)
            .on_press(move |s: &mut Workspace| {
                crate::remote::select(s, &target);
            }),
        );
    }
    if shown == 0 {
        rows = rows.child(
            k.interactive(
                row()
                    .w_full()
                    .center_y()
                    .gap(8.0)
                    .px(4.0)
                    .py(4.0)
                    .radius(6.0)
                    .child(icon(&ui::SERVER, 13.0).color(k.c.icon_muted).a11y_hidden())
                    .child(
                        text("Add remote VM…")
                            .font(k.ty.small)
                            .color(k.c.text_muted)
                            .grow(1.0),
                    ),
            )
            .role("button")
            .label("Add remote VM")
            .focusable(true)
            .on_press(|s: &mut Workspace| {
                live_mut(s).panels.set_overlay(Some(Screen::CreateVm));
            }),
        );
    }
    rows
}

fn machine_row(
    glyph: &'static rune::Icon,
    name: &str,
    memory: &str,
    history: Option<&[f32]>,
    right: &str,
    right_tone: Color,
    k: &Kit,
) -> El {
    row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(icon(glyph, 13.0).color(k.c.icon_muted).a11y_hidden())
        .child(
            text(name)
                .font(k.ty.ui)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            text(memory)
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        )
        .children(history.filter(|h| !h.is_empty()).map(|h| sparkline(h, k)))
        .child(
            row()
                .min_w(30.0)
                .justify_end()
                .child(text(right).font(k.ty.small_medium).color(right_tone)),
        )
}

fn sparkline(values: &[f32], k: &Kit) -> El {
    let mut bars = row().w(44.0).h(16.0).gap(1.5).items_end();
    let count = values.len().min(10);
    let slice = &values[values.len() - count..];
    for &pct in slice {
        let h = ((pct / 100.0).clamp(0.0, 1.0) * 14.0).max(2.0);
        bars = bars.child(
            el().grow(1.0)
                .min_w(1.0)
                .h(h)
                .radius(1.0)
                .fill(k.alpha(k.c.text_accent, 0.8)),
        );
    }
    bars
}
