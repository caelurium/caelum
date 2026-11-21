use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{Activity, Checkout, Job, Lifetime, Monitor, Phase, Process};
use rune::{El, Tx, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub(super) fn wide() -> bool {
    rune::memo::window_layout("main-pane").is_none_or(|r| r[2] >= 520.0)
}
fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub(super) fn sync(l: &mut Live, tx: &mut Tx, now: Instant) {
    if l.fixture || l.evaluation {
        return;
    }
    let unix_now = unix_now();
    let deadlines: Vec<_> = l
        .app
        .remote
        .saved
        .checkouts
        .iter()
        .filter_map(|(id, checkout)| {
            let Phase::Ready { machine, .. } = &checkout.phase else {
                return None;
            };
            Some((
                id.clone(),
                machine.key.clone(),
                machine.auto_stop_unix?,
                checkout.lifetime,
            ))
        })
        .collect();
    let active_deadlines: std::collections::HashSet<_> = deadlines
        .iter()
        .map(|(_, machine, deadline, _)| (machine.clone(), *deadline))
        .collect();
    l.remote
        .checked_deadlines
        .retain(|key| active_deadlines.contains(key));
    let mut next_auto_stop = None;
    for (id, machine, deadline, lifetime) in deadlines {
        if l.app.remote.jobs.contains_key(&id)
            || matches!(l.app.remote.lifetimes.get(&id), Some(Job::Busy(_)))
        {
            continue;
        }
        let project = caelum_core::remote::native::project_key(&id);
        let active = l
            .app
            .sessions
            .iter()
            .any(|session| session.scope.project == project && session.working());
        let at = if active {
            deadline.saturating_sub(300)
        } else {
            deadline.saturating_add(6)
        };
        let remaining = at.saturating_sub(unix_now);
        if remaining > 0 {
            let remaining = Duration::from_secs(remaining as u64);
            next_auto_stop =
                Some(next_auto_stop.map_or(remaining, |next: Duration| next.min(remaining)));
            continue;
        }
        let key = (machine, deadline);
        if l.remote.checked_deadlines.insert(key) {
            if active {
                l.app.set_vm_lifetime(&id, lifetime);
            } else {
                l.app.refresh_remote(&id);
            }
        }
    }
    if let Some(at) = next_auto_stop {
        tx.wake_after(at);
    }
    let inspected = super::popup::owner(l);
    let id = if l.app.window_background {
        None
    } else if inspected.is_some() {
        inspected
    } else if l.panels.screen == Screen::Remote && l.panels.overlay.is_none() {
        l.app.remote.saved.selected.as_deref()
    } else if l.panels.screen == Screen::Conversation && l.panels.overlay.is_none() {
        super::native::selected(l)
    } else {
        None
    };
    let Some((id, checkout)) = id.and_then(|id| l.app.remote.saved.checkouts.get_key_value(id))
    else {
        l.app.monitor_remote(None, now);
        return;
    };
    let visible = (inspected.is_some() || (wide() && checkout.pinned_stats)).then(|| id.clone());
    if let Some(at) = l.app.monitor_remote(visible.as_deref(), now) {
        tx.wake_after(at.saturating_duration_since(now));
    }
}

fn process(p: &Process) -> String {
    format!("{} · {:.0}% CPU", p.name, p.cpu_percent)
}

pub(super) fn pinned(s: &Workspace, k: &Kit, id: &str, c: &Checkout) -> Option<El> {
    if !c.pinned_stats || !wide() {
        return None;
    }
    let sample = live(s).app.remote.monitor.sample(id)?;
    let Activity::Live(Some(p)) = &sample.activity else {
        return None;
    };
    Some(
        ui::caption(process(p), k)
            .truncate()
            .shrink()
            .min_w(0.0)
            .key("remote-pinned-stats"),
    )
}

fn info(label: &str, value: impl Into<String>, k: &Kit) -> El {
    row()
        .w_full()
        .gap(8.0)
        .child(ui::caption(label, k).w(76.0))
        .child(
            text(value.into())
                .font(k.ty.small_medium)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .min_w(0.0),
        )
}

pub(super) fn details(s: &Workspace, k: &Kit, id: &str, c: &Checkout) -> El {
    let r = &live(s).app.remote;
    let machine = c.machine().unwrap();
    let mut box_ = col()
        .w_full()
        .gap(8.0)
        .pb(8.0)
        .key("remote-machine-details")
        .child(
            row()
                .w_full()
                .center_y()
                .gap(8.0)
                .child(icon(&ui::SERVER, 18.0).color(k.c.icon_accent))
                .child(
                    col()
                        .min_w(0.0)
                        .grow(1.0)
                        .gap(2.0)
                        .child(
                            text(&machine.name)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .child(
                            ui::caption(format!("{} · {}", c.source.repository, machine.state), k)
                                .truncate(),
                        ),
                ),
        )
        .child(ui::divider(k));
    let resume = matches!(machine.state.as_str(), "stopped" | "archived");
    let ttl = if resume {
        "Stopped".into()
    } else {
        machine.auto_stop_unix.map_or_else(
            || "Off".into(),
            |t| {
                let left = t.saturating_sub(unix_now());
                if left <= 0 {
                    "Due now".into()
                } else {
                    let mins = left.div_euclid(60) + i64::from(left % 60 != 0);
                    match (mins / 60, mins % 60) {
                        (0, m) => format!("in {m} min"),
                        (h, 0) => format!("in {h} h"),
                        (h, m) => format!("in {h} h {m} min"),
                    }
                }
            },
        )
    };
    box_ = box_.child(info("Auto-stop", ttl, k)).child(info(
        "Active thread",
        super::status::status(r, id, c).label,
        k,
    ));
    if let Some(sample) = r.monitor.sample(id) {
        if let (Some(cpu), Some(ram)) = (sample.vcpu, sample.memory_gb) {
            box_ = box_.child(info("Capacity", format!("{cpu} vCPU · {ram} GB RAM"), k));
        }
        let (label, p) = match &sample.activity {
            Activity::Live(p) => ("Busy now", p.as_ref()),
            Activity::LastSample(p) => ("Last sample", p.as_ref()),
            Activity::Unavailable => ("Processes", None),
        };
        let value = p.map_or_else(
            || "No process sample".into(),
            |p| {
                let cpu = process(p);
                p.memory_mb
                    .map_or(cpu.clone(), |ram| format!("{cpu} · {ram:.0} MB RAM"))
            },
        );
        box_ = box_.child(info(label, value, k));
    } else if let Monitor::Failed { owner, error } = &r.monitor
        && owner == id
    {
        box_ = box_.child(ui::caption(error, k).wrap().color(k.c.danger.fg));
    } else if matches!(c.phase, Phase::Ready { .. }) {
        box_ = box_.child(ui::caption("Reading machine stats…", k));
    }
    let a = id.to_owned();
    let b = a.clone();
    let owner = a.clone();
    if !resume {
        let mut choices = row()
            .w_full()
            .flex_wrap()
            .center_y()
            .gap(4.0)
            .child(ui::caption("Set from now", k));
        for time in [
            Lifetime::TwoHours,
            Lifetime::EightHours,
            Lifetime::Day,
            Lifetime::UntilStopped,
        ] {
            let owner = id.to_owned();
            choices = choices.child(
                ui::Button::new(time.label())
                    .variant(ButtonVariant::GhostMuted)
                    .disabled(!live(s).app.can_set_vm_lifetime(id))
                    .on_press(move |s: &mut Workspace| {
                        live_mut(s).app.set_vm_lifetime(&owner, time)
                    })
                    .build(k)
                    .key(format!("set-vm-lifetime-{}", time.label())),
            );
        }
        box_ = box_.child(ui::divider(k)).child(choices).child(
            ui::caption(
                "Active native Codex turns renew auto-stop using the saved lifetime.",
                k,
            )
            .wrap(),
        );
    }
    if let Some(job) = r.lifetimes.get(id) {
        box_ = box_.child(
            ui::caption(
                match job {
                    Job::Failed(e) => e,
                    _ => "Updating auto-stop…",
                },
                k,
            )
            .wrap(),
        );
    }
    box_.child(
        ui::Button::new(if c.pinned_stats {
            "Unpin stats"
        } else {
            "Pin stats to VM composer"
        })
        .variant(ButtonVariant::GhostMuted)
        .on_press(move |s: &mut Workspace| {
            if let Some(c) = live_mut(s).app.remote.saved.checkouts.get_mut(&owner) {
                c.pinned_stats = !c.pinned_stats;
            }
        })
        .build(k)
        .key("remote-pin-stats"),
    )
    .child(
        row()
            .w_full()
            .flex_wrap()
            .gap(4.0)
            .children((live(s).panels.screen == Screen::Remote).then(|| {
                ui::Button::new("Changes")
                    .on_press(|s: &mut Workspace| {
                        live_mut(s).panels.overlay = None;
                        super::open_changes(s);
                    })
                    .build(k)
            }))
            .child(
                ui::Button::new("Refresh")
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        l.app.remote.monitor = Monitor::Off;
                        l.app.refresh_remote(&a);
                    })
                    .build(k),
            )
            .child(
                ui::Button::new(if resume { "Resume" } else { "Stop machine" })
                    .on_press(move |s: &mut Workspace| live_mut(s).app.power_remote(&b, resume))
                    .build(k),
            ),
    )
}
