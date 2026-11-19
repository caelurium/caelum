use crate::{
    Workspace,
    live::{Live, live, live_mut},
    panels::Screen,
    ui,
};
use caelum_core::remote::{Activity, Checkout, Monitor, Phase, Process};
use rune::{El, Tx, col, row};
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
    let shown =
        l.panels.screen == Screen::Remote && l.panels.overlay.is_none() && !l.app.window_background;
    let checkout = l
        .app
        .remote
        .saved
        .selected
        .as_ref()
        .and_then(|id| l.app.remote.saved.checkouts.get(id));
    let visible =
        shown && (l.remote.details || (wide() && checkout.is_some_and(|c| c.pinned_stats)));
    // A collapsed badge doesn't sample processes, but must still notice its known auto-stop.
    if shown
        && let Some(Checkout {
            phase: Phase::Ready { machine, .. },
            ..
        }) = checkout
        && let Some(stops) = machine.auto_stop_unix
        && !l
            .remote
            .checked_deadline
            .as_ref()
            .is_some_and(|(m, t)| m == &machine.key && *t == stops)
    {
        let remaining = stops.saturating_add(6).saturating_sub(unix_now());
        if remaining > 0 {
            tx.wake_after(Duration::from_secs(remaining as u64));
        } else if let Some(id) = l.app.remote.saved.selected.clone() {
            l.remote.checked_deadline = Some((machine.key.clone(), stops));
            l.app.refresh_remote(&id);
        }
    }
    if let Some(at) = l.app.monitor_remote(visible, now) {
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
        .child(ui::caption(value, k).wrap().min_w(0.0))
}

pub(super) fn details(s: &Workspace, k: &Kit, id: &str, c: &Checkout) -> El {
    let r = &live(s).app.remote;
    let machine = c.machine().unwrap();
    let mut box_ = col()
        .w_full()
        .gap(5.0)
        .pb(8.0)
        .key("remote-machine-details")
        .child(info(
            "Machine",
            format!("{} · {}", machine.name, machine.state),
            k,
        ));
    if let Some(sample) = r.monitor.sample(id) {
        if let (Some(cpu), Some(ram)) = (sample.vcpu, sample.memory_gb) {
            box_ = box_.child(info("Capacity", format!("{cpu} vCPU · {ram} GB RAM"), k));
        }
        let ttl = machine.auto_stop_unix.map_or_else(
            || "Off".into(),
            |t| {
                let left = t.saturating_sub(unix_now());
                if left <= 0 {
                    "Due now".into()
                } else {
                    format!("in {} min", left.div_euclid(60) + i64::from(left % 60 != 0))
                }
            },
        );
        box_ = box_.child(info("Auto-stop", ttl, k));
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
    let resume = matches!(machine.state.as_str(), "stopped" | "archived");
    box_.child(
        ui::Button::new(if c.pinned_stats {
            "Unpin stats"
        } else {
            "Pin stats to composer"
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
            .child(
                ui::Button::new("Changes")
                    .on_press(super::open_changes)
                    .build(k),
            )
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
