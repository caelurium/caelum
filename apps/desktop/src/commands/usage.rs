use crate::{
    Workspace,
    live::live,
    ui::{self, Button},
};
use caelum_core::application::Task;
use rune::{El, col, row, text};
use rune_kit::{ButtonVariant, Kit};

pub(super) fn view(s: &Workspace, k: &Kit, index: usize) -> El {
    let l = live(s);
    let scope = &l.app.sessions[index].scope;
    let project = &scope.project;
    let provider = scope.provider;
    let task = Task::Usage {
        project: project.clone(),
        provider,
    };
    let busy = l.app.services.busy(&task);
    let value = l
        .app
        .services
        .checkouts
        .get(project)
        .and_then(|c| c.usage.get(&provider));
    let mut body = col().w_full().gap(12.0);
    if l.app.sessions[index].conversation.used_tokens.is_some() {
        body = body.child(
            ui::card(k).child(
                row()
                    .w_full()
                    .center_y()
                    .justify_between()
                    .gap(8.0)
                    .child(
                        text("Current session")
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base),
                    )
                    .child(crate::activity::context(s, k, index, true)),
            ),
        );
    }
    if let Some(value) = value {
        if provider == caelum_core::application::Provider::Antigravity {
            body = antigravity_usage_body(body, value, k);
        } else {
            body = opencode_usage_body(body, value, k);
        }
    } else {
        let label = if provider == caelum_core::application::Provider::Antigravity {
            "Loading Antigravity usage"
        } else {
            "Loading OpenCode Go usage"
        };
        body = body.child(ui::card(k).child(if busy {
            ui::skeleton(label, 3, k)
        } else {
            ui::empty(
                "No usage reported",
                "Refresh to check your subscription allowances.",
                k,
            )
        }));
    }
    super::service_status(body, s, k, task, value.is_some())
}

fn usage_bar(label: &str, fraction: f64, fill: rune::Color, k: &Kit) -> El {
    let clamped = fraction.clamp(0.0, 1.0) as f32;
    let mut track = row()
        .w_full()
        .h(8.0)
        .radius(4.0)
        .fill(k.c.layer_03)
        .border(1.0, k.alpha(k.c.text_base, 0.05))
        .clip_rounded(4.0)
        .role("progressbar")
        .label(format!("{label} used"));
    if clamped > 0.0 {
        track = track.child(
            rune::el()
                .w_frac(clamped)
                .min_w(4.0)
                .h_full()
                .radius(4.0)
                .fill(fill),
        );
    }
    track
}

fn usage_bucket(
    label: &str,
    used_pct: f64,
    remaining_label: String,
    limited: bool,
    detail: Option<String>,
    k: &Kit,
) -> El {
    let used = used_pct.clamp(0.0, 100.0);
    let fill = if limited || used >= 95.0 {
        k.c.danger.fg
    } else if used >= 75.0 {
        k.c.warning.fg
    } else {
        k.c.text_accent
    };
    let status = if limited {
        text("Limit reached")
            .font(k.ty.small)
            .color(k.c.danger.fg)
            .px(6.0)
            .py(1.0)
            .radius(4.0)
            .fill(k.alpha(k.c.danger.fg, 0.14))
    } else {
        text(format!("{used:.0}%"))
            .font(k.ty.mono)
            .color(if used >= 75.0 { fill } else { k.c.text_base })
    };
    let detail = detail.filter(|d| !d.is_empty());
    let mut bucket = col()
        .w_full()
        .gap(6.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.0)
                .child(
                    text(label)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .child(status),
        )
        .child(usage_bar(label, used / 100.0, fill, k));
    if detail.is_some() || !remaining_label.is_empty() {
        bucket = bucket.child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.0)
                .child(
                    detail
                        .map(|d| ui::caption(d, k).truncate().grow(1.0).min_w(0.0))
                        .unwrap_or_else(|| rune::el().grow(1.0)),
                )
                .children((!remaining_label.is_empty()).then(|| ui::caption(remaining_label, k))),
        );
    }
    bucket
}

fn antigravity_usage_body(mut body: El, value: &serde_json::Value, k: &Kit) -> El {
    let tier = value["tier"]["name"].as_str().filter(|s| !s.is_empty());
    let summary = value["description"].as_str().filter(|s| !s.is_empty());
    if tier.is_some() || summary.is_some() {
        let mut header = ui::card(k).gap(4.0);
        if let Some(tier) = tier {
            header = header.child(
                row()
                    .w_full()
                    .center_y()
                    .justify_between()
                    .gap(8.0)
                    .child(ui::caption("Plan", k))
                    .child(text(tier).font(k.ty.ui_medium).color(k.c.text_base)),
            );
        }
        if let Some(summary) = summary {
            header = header.child(ui::caption(summary, k));
        }
        body = body.child(header);
    }
    let mut reported = false;
    for group in value["groups"].as_array().into_iter().flatten() {
        let name = group["name"].as_str().unwrap_or("Models");
        let mut section = ui::card(k).gap(10.0).child(
            col()
                .w_full()
                .gap(2.0)
                .child(text(name).font(k.ty.ui_medium).color(k.c.text_base))
                .children(
                    group["description"]
                        .as_str()
                        .filter(|s| !s.is_empty())
                        .map(|desc| ui::caption(desc, k)),
                ),
        );
        let mut first_bucket = true;
        for bucket in group["buckets"].as_array().into_iter().flatten() {
            let Some(remaining) = bucket["remaining_fraction"].as_f64() else {
                continue;
            };
            reported = true;
            if first_bucket {
                section = section.child(ui::divider(k));
                first_bucket = false;
            }
            let remaining_pct = (remaining * 100.0).clamp(0.0, 100.0);
            let used_pct = (100.0 - remaining_pct).max(0.0);
            let label = bucket["name"]
                .as_str()
                .or(bucket["window"].as_str())
                .unwrap_or("Quota");
            section = section.child(usage_bucket(
                label,
                used_pct,
                format!("{remaining_pct:.0}% remaining"),
                remaining_pct <= 0.0,
                bucket["description"].as_str().map(str::to_owned),
                k,
            ));
        }
        if !first_bucket {
            body = body.child(section);
        }
    }
    if !reported {
        body = body.child(ui::card(k).child(ui::caption("No Antigravity quotas reported", k)));
    }
    if let Some(url) = value["tier"]["upgradeUri"]
        .as_str()
        .filter(|s| !s.is_empty())
    {
        let url = url.to_owned();
        body = body.child(
            Button::new("Manage plan")
                .variant(ButtonVariant::Outline)
                .on_press_tx(move |_: &mut Workspace, tx| {
                    let _ = tx.open_url(&url);
                })
                .build(k),
        );
    }
    body
}

fn opencode_usage_body(mut body: El, value: &serde_json::Value, k: &Kit) -> El {
    let workspace = value["workspace"]["name"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("OpenCode Go");
    let mut allowances = ui::card(k)
        .gap(12.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.0)
                .child(ui::caption("Workspace", k))
                .child(text(workspace).font(k.ty.ui_medium).color(k.c.text_base)),
        )
        .child(ui::divider(k));
    let mut reported = false;
    for (name, title) in [
        ("rolling", "Rolling"),
        ("weekly", "Weekly"),
        ("monthly", "Monthly"),
    ] {
        let allowance = &value["go"][name];
        if let Some(percent) = allowance["percent"].as_f64() {
            reported = true;
            let remaining = (100.0 - percent).max(0.0);
            let reset = allowance["resetsAt"].as_u64().map(|reset| {
                format!(
                    "Resets in {}",
                    caelum_core::application::presentation::duration(
                        reset.saturating_sub(caelum_core::application::presentation::now())
                    )
                )
            });
            allowances = allowances.child(usage_bucket(
                title,
                percent,
                format!("{remaining:.0}% remaining"),
                allowance["limited"] == true,
                reset,
                k,
            ));
        }
    }
    if !reported {
        allowances = allowances.child(ui::caption("No OpenCode Go allowances reported", k));
    }
    body = body.child(allowances);
    if let Some(budgets) = value["budgets"].as_array().filter(|b| !b.is_empty()) {
        let mut budget_card = ui::card(k)
            .gap(10.0)
            .child(
                text("Console budgets")
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(ui::divider(k));
        for budget in budgets {
            let spent = budget["spent"].as_f64().unwrap_or(0.0);
            let limit = budget["limit"].as_f64().unwrap_or(0.0);
            let pct = if limit > 0.0 {
                (spent / limit * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let name = budget["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("Monthly budget");
            budget_card = budget_card.child(usage_bucket(
                name,
                pct,
                format!("${spent:.2} / ${limit:.2}"),
                limit > 0.0 && spent >= limit,
                None,
                k,
            ));
        }
        body = body.child(budget_card);
    }
    if let Some(url) = value["workspace"]["usageURL"].as_str() {
        let url = url.to_owned();
        body = body.child(
            Button::new("Open usage dashboard")
                .variant(ButtonVariant::Outline)
                .on_press_tx(move |_: &mut Workspace, tx| {
                    let _ = tx.open_url(&url);
                })
                .build(k),
        );
    }
    body
}
