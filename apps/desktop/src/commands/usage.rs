use crate::{
    Workspace,
    live::live,
    ui::{self, Button},
};
use caelum_core::application::Task;
use rune::{El, col, row, text};
use rune_kit::{ButtonVariant, Kit, Progress};

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
    let mut body = ui::card(k);
    if l.app.sessions[index].conversation.used_tokens.is_some() {
        body = body
            .child(
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
            )
            .child(ui::divider(k));
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
        body = body.child(if busy {
            ui::skeleton(label, 3, k)
        } else {
            ui::empty(
                "No usage reported",
                "Refresh to check your subscription allowances.",
                k,
            )
        });
    }
    super::service_status(body, s, k, task, value.is_some())
}

fn usage_bucket(
    label: &str,
    used_pct: f64,
    remaining_pct: f64,
    limited: bool,
    detail: Option<String>,
    k: &Kit,
) -> El {
    let status = if limited {
        text("Limit reached").font(k.ty.small).color(k.c.danger.fg)
    } else {
        ui::caption(format!("{used_pct:.0}% used · {remaining_pct:.0}% left"), k)
    };
    col()
        .w_full()
        .gap(5.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.0)
                .child(text(label).font(k.ty.ui_medium).color(k.c.text_base))
                .child(status),
        )
        .child(
            Progress::new(Some((used_pct / 100.0).clamp(0.0, 1.0) as f32))
                .label(format!("{label} used"))
                .build(k),
        )
        .children(detail.filter(|d| !d.is_empty()).map(|d| ui::caption(d, k)))
}

fn antigravity_usage_body(mut body: El, value: &serde_json::Value, k: &Kit) -> El {
    if let Some(tier) = value["tier"]["name"].as_str().filter(|s| !s.is_empty()) {
        body = body.child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .gap(8.0)
                .child(ui::caption("Plan", k))
                .child(text(tier).font(k.ty.ui_medium).color(k.c.text_base)),
        );
    }
    let mut reported = false;
    for group in value["groups"].as_array().into_iter().flatten() {
        let name = group["name"].as_str().unwrap_or("Models");
        let mut section = col()
            .w_full()
            .gap(8.0)
            .child(text(name).font(k.ty.ui_medium).color(k.c.text_base));
        if let Some(desc) = group["description"].as_str().filter(|s| !s.is_empty()) {
            section = section.child(ui::caption(desc, k));
        }
        for bucket in group["buckets"].as_array().into_iter().flatten() {
            let Some(remaining) = bucket["remaining_fraction"].as_f64() else {
                continue;
            };
            reported = true;
            let remaining_pct = (remaining * 100.0).clamp(0.0, 100.0);
            let used_pct = (100.0 - remaining_pct).max(0.0);
            let label = bucket["name"]
                .as_str()
                .or(bucket["window"].as_str())
                .unwrap_or("Quota");
            section = section.child(usage_bucket(
                label,
                used_pct,
                remaining_pct,
                remaining_pct <= 0.0,
                bucket["description"].as_str().map(str::to_owned),
                k,
            ));
        }
        body = body.child(ui::divider(k)).child(section);
    }
    if !reported {
        body = body.child(ui::caption("No Antigravity quotas reported", k));
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
    body = body
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
            let reset = allowance["resetsAt"].as_u64().map(|reset| {
                format!(
                    "Resets in {}",
                    caelum_core::application::presentation::duration(
                        reset.saturating_sub(caelum_core::application::presentation::now())
                    )
                )
            });
            body = body.child(usage_bucket(
                title,
                percent,
                (100.0 - percent).max(0.0),
                allowance["limited"] == true,
                reset,
                k,
            ));
        }
    }
    if !reported {
        body = body.child(ui::caption("No OpenCode Go allowances reported", k));
    }
    if let Some(budgets) = value["budgets"].as_array() {
        for budget in budgets {
            body = body.child(ui::caption(
                format!(
                    "Console budget · ${:.2} / ${:.2}",
                    budget["spent"].as_f64().unwrap_or(0.0),
                    budget["limit"].as_f64().unwrap_or(0.0)
                ),
                k,
            ));
        }
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
