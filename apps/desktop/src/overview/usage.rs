use super::RESET;
use crate::{Workspace, live::live, ui};
use caelum_core::application::{Provider, Scope, Task};
use rune::{El, Icon, col, el, icon, row, text};
use rune_kit::Kit;

struct UsageEntry {
    scope: Scope,
    glyph: &'static Icon,
    label: String,
    sublabel: String,
    reset: String,
    tooltip: String,
    percent: f64,
    limited: bool,
}

fn short_window(window: &str) -> String {
    match window.trim().to_ascii_lowercase().as_str() {
        "weekly" | "week" => "7d".into(),
        "monthly" | "month" => "30d".into(),
        "daily" | "day" => "24h".into(),
        "five_hour" | "5_hour" | "5h" => "5h".into(),
        other => other.to_owned(),
    }
}

fn window_rank(window: &str) -> usize {
    match window {
        "5h" => 0,
        "24h" => 1,
        "7d" => 2,
        "30d" => 3,
        _ => 4,
    }
}

fn short_group_name(name: &str) -> String {
    let trimmed = name.trim();
    trimmed
        .strip_suffix(" Models")
        .or_else(|| trimmed.strip_suffix(" models"))
        .unwrap_or(trimmed)
        .to_owned()
}

fn compact_duration_ms(ms: u64) -> String {
    let mins = ms / 60_000;
    if mins >= 24 * 60 {
        let days = (mins + 12 * 60) / (24 * 60);
        format!("{days}d")
    } else if mins >= 60 {
        let hours = (mins + 30) / 60;
        if hours >= 24 {
            "1d".into()
        } else {
            format!("{hours}h")
        }
    } else if mins >= 1 {
        format!("{mins}m")
    } else {
        "<1m".into()
    }
}

fn parse_iso8601_utc_ms(raw: &str) -> Option<u64> {
    let raw = raw.trim().strip_suffix('Z')?;
    let (date, time) = raw.split_once('T')?;
    let mut date_parts = date.split('-');
    let year: i64 = date_parts.next()?.parse().ok()?;
    let month: u32 = date_parts.next()?.parse().ok()?;
    let day: u32 = date_parts.next()?.parse().ok()?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let time = time.split('.').next()?;
    let mut time_parts = time.split(':');
    let hour: u64 = time_parts.next()?.parse().ok()?;
    let minute: u64 = time_parts.next()?.parse().ok()?;
    let second: u64 = time_parts.next()?.parse().ok()?;
    if time_parts.next().is_some() || hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64;
    let m = month as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i64 - 719_468;
    if days < 0 {
        return None;
    }
    let secs = (days as u64) * 86_400 + hour * 3_600 + minute * 60 + second;
    Some(secs.saturating_mul(1_000))
}

fn compact_description_reset(desc: &str) -> Option<String> {
    let words: Vec<&str> = desc
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    for pair in words.windows(2) {
        let Ok(value) = pair[0].parse::<u64>() else {
            continue;
        };
        let suffix = match pair[1].to_ascii_lowercase().as_str() {
            "d" | "day" | "days" => "d",
            "h" | "hr" | "hrs" | "hour" | "hours" => "h",
            "m" | "min" | "mins" | "minute" | "minutes" => "m",
            "w" | "wk" | "wks" | "week" | "weeks" => "w",
            _ => continue,
        };
        return Some(format!("{value}{suffix}"));
    }
    None
}

fn antigravity_reset(bucket: &serde_json::Value, now: u64) -> String {
    if let Some(reset_ms) = bucket["reset_time"]
        .as_str()
        .and_then(parse_iso8601_utc_ms)
        .filter(|&at| at > now)
    {
        return compact_duration_ms(reset_ms - now);
    }
    bucket["description"]
        .as_str()
        .and_then(compact_description_reset)
        .unwrap_or_default()
}

fn usage_checkout(s: &Workspace, provider: Provider) -> Option<(&str, &serde_json::Value)> {
    let l = live(s);
    if let Some(active) = l.app.active_project.as_deref()
        && let Some(value) = l
            .app
            .services
            .checkouts
            .get(active)
            .and_then(|c| c.usage.get(&provider))
    {
        return Some((active, value));
    }
    let mut keys: Vec<&String> = l.app.services.checkouts.keys().collect();
    keys.sort();
    for path in keys {
        if let Some(value) = l.app.services.checkouts[path].usage.get(&provider) {
            return Some((path.as_str(), value));
        }
    }
    None
}

fn collect_usage(s: &Workspace) -> Vec<UsageEntry> {
    let now = live(s).feed.displayed_at();
    let mut ag_primary: Vec<(usize, usize, UsageEntry)> = Vec::new();
    let mut ag_secondary: Vec<(usize, usize, UsageEntry)> = Vec::new();
    if let Some((project, value)) = usage_checkout(s, Provider::Antigravity) {
        let scope = Scope {
            project: project.to_owned(),
            provider: Provider::Antigravity,
        };
        for (group_idx, group) in value["groups"].as_array().into_iter().flatten().enumerate() {
            let group_name = group["name"].as_str().unwrap_or("").trim();
            let mut buckets = Vec::new();
            for bucket in group["buckets"].as_array().into_iter().flatten() {
                let Some(remaining) = bucket["remaining_fraction"].as_f64() else {
                    continue;
                };
                let remaining_pct = (remaining * 100.0).clamp(0.0, 100.0);
                let used = (100.0 - remaining_pct).max(0.0);
                let window = short_window(bucket["window"].as_str().unwrap_or(""));
                let rank = window_rank(&window);
                let reset = antigravity_reset(bucket, now);
                let desc = bucket["description"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                let label = if !group_name.is_empty() {
                    short_group_name(group_name)
                } else {
                    bucket["name"].as_str().unwrap_or("Antigravity").to_owned()
                };
                buckets.push((
                    rank,
                    label,
                    window,
                    reset,
                    desc,
                    used,
                    remaining_pct,
                    remaining <= 0.0,
                ));
            }
            if buckets.is_empty() {
                continue;
            }
            buckets.sort_by_key(|b| b.0);
            let label = buckets[0].1.clone();
            let glyph = crate::models::brand_glyph(&label).unwrap_or(&crate::ORBIT);
            let mut tooltip = format!("Antigravity · {label}");
            for (_, _, window, _, desc, used, remaining_pct, _) in &buckets {
                tooltip.push('\n');
                if window.is_empty() {
                    tooltip.push_str(&format!("{used:.0}% used ({remaining_pct:.0}% left)"));
                } else {
                    tooltip.push_str(&format!(
                        "{window}: {used:.0}% used ({remaining_pct:.0}% left)"
                    ));
                }
                if !desc.is_empty() {
                    tooltip.push_str(" · ");
                    tooltip.push_str(desc);
                }
            }
            tooltip.push_str("\nClick to view full usage");
            let primary_idx = buckets
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.5.total_cmp(&b.5).then_with(|| b.0.cmp(&a.0)))
                .map_or(0, |(idx, _)| idx);
            for (idx, (rank, label, window, reset, _, used, _, limited)) in
                buckets.into_iter().enumerate()
            {
                let entry = UsageEntry {
                    scope: scope.clone(),
                    glyph,
                    label,
                    sublabel: window,
                    reset,
                    tooltip: tooltip.clone(),
                    percent: used,
                    limited,
                };
                if idx == primary_idx {
                    ag_primary.push((group_idx, rank, entry));
                } else {
                    ag_secondary.push((group_idx, rank, entry));
                }
            }
        }
    }

    let mut oc_entries: Vec<(usize, &'static str, &'static str, UsageEntry)> = Vec::new();
    if let Some((project, value)) = usage_checkout(s, Provider::OpenCode) {
        let scope = Scope {
            project: project.to_owned(),
            provider: Provider::OpenCode,
        };
        let glyph = crate::models::brand_glyph("opencode").unwrap_or(&crate::ORBIT);
        let windows = [
            ("rolling", "Rolling", "Go", 0usize),
            ("weekly", "Weekly", "Go 7d", 1usize),
            ("monthly", "Monthly", "Go 30d", 2usize),
        ];
        let mut raw = Vec::new();
        for (key, multi, single, rank) in windows {
            let allowance = &value["go"][key];
            let Some(percent) = allowance["percent"].as_f64() else {
                continue;
            };
            let used = percent.clamp(0.0, 100.0);
            let remaining_pct = (100.0 - used).max(0.0);
            let reset_ms = allowance["resetsAt"]
                .as_u64()
                .filter(|&at| at > now)
                .map(|at| at - now);
            let reset = reset_ms.map(compact_duration_ms).unwrap_or_default();
            let reset_long = reset_ms
                .map(caelum_core::application::presentation::duration)
                .unwrap_or_default();
            raw.push((
                rank,
                multi,
                single,
                used,
                remaining_pct,
                reset,
                reset_long,
                allowance["limited"] == true,
            ));
        }
        if !raw.is_empty() {
            let mut tooltip = String::from("OpenCode Go");
            for (_, multi, _, used, remaining_pct, _, reset_long, _) in &raw {
                tooltip.push_str(&format!(
                    "\n{multi}: {used:.0}% used ({remaining_pct:.0}% left)"
                ));
                if !reset_long.is_empty() {
                    tooltip.push_str(&format!(" · Resets in {reset_long}"));
                }
            }
            tooltip.push_str("\nClick to view full usage");
            for (rank, multi, single, used, _, reset, _, limited) in raw {
                oc_entries.push((
                    rank,
                    multi,
                    single,
                    UsageEntry {
                        scope: scope.clone(),
                        glyph,
                        label: "OpenCode".into(),
                        sublabel: single.into(),
                        reset,
                        tooltip: tooltip.clone(),
                        percent: used,
                        limited,
                    },
                ));
            }
        }
    }

    let has_oc = !oc_entries.is_empty();
    let ag_primary_cap = if has_oc { 2 } else { 3 };
    let mut ag_chosen: Vec<(usize, usize, UsageEntry)> =
        ag_primary.into_iter().take(ag_primary_cap).collect();
    let reserve_oc = usize::from(has_oc);
    for extra in ag_secondary {
        if ag_chosen.len() + reserve_oc >= 3 {
            break;
        }
        ag_chosen.push(extra);
    }
    ag_chosen.sort_by_key(|(group_idx, rank, _)| (*group_idx, *rank));

    let oc_cap = 3usize.saturating_sub(ag_chosen.len());
    let mut oc_chosen = Vec::new();
    if oc_cap > 0 && !oc_entries.is_empty() {
        let primary_idx = oc_entries
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| {
                a.3.percent
                    .total_cmp(&b.3.percent)
                    .then_with(|| b.0.cmp(&a.0))
            })
            .map_or(0, |(idx, _)| idx);
        oc_chosen.push(oc_entries.remove(primary_idx));
        for extra in oc_entries {
            if oc_chosen.len() >= oc_cap {
                break;
            }
            oc_chosen.push(extra);
        }
        oc_chosen.sort_by_key(|(rank, _, _, _)| *rank);
        let multiple = oc_chosen.len() > 1;
        for (_, multi, single, entry) in &mut oc_chosen {
            entry.sublabel = if multiple { *multi } else { *single }.into();
        }
    }

    ag_chosen
        .into_iter()
        .map(|(_, _, entry)| entry)
        .chain(oc_chosen.into_iter().map(|(_, _, _, entry)| entry))
        .collect()
}

pub(super) fn card(s: &Workspace, compact: bool, k: &Kit) -> El {
    let entries = collect_usage(s);
    if entries.is_empty() {
        let l = live(s);
        let loading = l.app.projects.first().is_some_and(|p| {
            [Provider::Antigravity, Provider::OpenCode]
                .iter()
                .any(|&provider| {
                    l.app.services.busy(&Task::Usage {
                        project: p.path.clone(),
                        provider,
                    })
                })
        });
        return col()
            .key("overview-card-usage")
            .w_full()
            .h_full()
            .justify_center()
            .gap(6.0)
            .child(ui::caption(
                if loading {
                    "Checking provider quotas…"
                } else {
                    "No provider quotas reported yet."
                },
                k,
            ))
            .child(ui::caption("Run /usage or refresh to update.", k));
    }
    let any_reset = entries.iter().any(|entry| !entry.reset.is_empty());
    let mut rows = col()
        .key("overview-card-usage")
        .w_full()
        .h_full()
        .justify_center()
        .gap(if compact { 4.0 } else { 6.0 });
    for entry in entries {
        let fill = if entry.limited || entry.percent >= 90.0 {
            k.c.danger.fg
        } else if entry.percent >= 75.0 {
            k.c.warning.fg
        } else {
            k.alpha(k.c.text_base, 0.85)
        };
        let pct_tone = if entry.limited || entry.percent >= 90.0 {
            k.c.danger.fg
        } else if entry.percent >= 75.0 {
            k.c.warning.fg
        } else {
            k.c.text_muted
        };
        let frac = (entry.percent / 100.0).clamp(0.0, 1.0) as f32;
        let scope = entry.scope.clone();
        let aria = if entry.sublabel.is_empty() {
            format!("Open {} usage ({:.0}%)", entry.label, entry.percent)
        } else {
            format!(
                "Open {} {} usage ({:.0}%)",
                entry.label, entry.sublabel, entry.percent
            )
        };
        rows =
            rows.child(
                k.interactive(
                    row()
                        .w_full()
                        .center_y()
                        .gap(if compact { 5.0 } else { 8.0 })
                        .px(if compact { 2.0 } else { 4.0 })
                        .py(if compact { 3.0 } else { 5.0 })
                        .radius(6.0)
                        .child(
                            row().w(if compact { 14.0 } else { 15.0 }).center().child(
                                icon(entry.glyph, if compact { 12.0 } else { 13.0 })
                                    .color(k.c.icon_muted)
                                    .a11y_hidden(),
                            ),
                        )
                        .child(
                            row()
                                .grow(1.0)
                                .min_w(0.0)
                                .center_y()
                                .gap(5.0)
                                .child(
                                    text(&entry.label)
                                        .font(if compact { k.ty.small_medium } else { k.ty.ui })
                                        .color(k.c.text_base)
                                        .truncate(),
                                )
                                .children((!compact && !entry.sublabel.is_empty()).then(|| {
                                    text(&entry.sublabel)
                                        .font(k.ty.small)
                                        .color(k.c.text_faint)
                                        .truncate()
                                })),
                        )
                        .children((!compact && any_reset).then(|| {
                            row()
                                .w(42.0)
                                .center_y()
                                .justify_end()
                                .gap(3.0)
                                .children((!entry.reset.is_empty()).then(|| {
                                    icon(&RESET, 10.5).color(k.c.text_faint).a11y_hidden()
                                }))
                                .children((!entry.reset.is_empty()).then(|| {
                                    text(&entry.reset).font(k.ty.small).color(k.c.text_faint)
                                }))
                        }))
                        .child(
                            row()
                                .w(if compact { 24.0 } else { 48.0 })
                                .h(4.0)
                                .radius(2.0)
                                .fill(k.alpha(k.c.text_base, 0.12))
                                .clip_rounded(2.0)
                                .children((frac > 0.0).then(|| {
                                    el().w_frac(frac).min_w(3.0).h_full().radius(2.0).fill(fill)
                                })),
                        )
                        .child(
                            row()
                                .w(if compact { 26.0 } else { 30.0 })
                                .justify_end()
                                .child(
                                    text(format!("{:.0}%", entry.percent))
                                        .font(k.ty.small_medium)
                                        .color(pct_tone),
                                ),
                        ),
                )
                .role("button")
                .label(aria)
                .help(entry.tooltip)
                .focusable(true)
                .focus_ring(1.5, 0.0, k.c.border_focus)
                .on_press(move |s: &mut Workspace| {
                    crate::commands::open_usage(s, scope.clone());
                }),
            );
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::live_mut;
    use serde_json::json;

    #[test]
    fn compact_duration_and_reset_parsing() {
        assert_eq!(compact_duration_ms((4 * 60 + 59) * 60_000), "5h");
        assert_eq!(compact_duration_ms(45 * 60_000), "45m");
        assert_eq!(compact_duration_ms(5 * 86_400_000), "5d");
        assert_eq!(
            compact_description_reset("Refreshes in 5 days.").as_deref(),
            Some("5d")
        );
        assert_eq!(
            compact_description_reset("Refreshes in 2 hours.").as_deref(),
            Some("2h")
        );
        let base = parse_iso8601_utc_ms("2026-11-23T12:00:00Z").unwrap();
        let later = parse_iso8601_utc_ms("2026-11-28T12:00:00Z").unwrap();
        assert_eq!(later - base, 5 * 86_400_000);
    }

    fn overview_state() -> Workspace {
        let mut ws = crate::evaluation::design();
        crate::evaluation::scene(&mut ws, "conversation-overview");
        ws
    }

    #[test]
    fn groups_usage_by_provider_without_interleaving() {
        let mut ws = overview_state();
        let now = live(&ws).feed.displayed_at();
        let checkout = live_mut(&mut ws)
            .app
            .services
            .checkouts
            .get_mut("/projects/rune")
            .unwrap();
        checkout.usage.insert(
            Provider::Antigravity,
            json!({
                "groups": [{
                    "name": "Gemini Models",
                    "buckets": [
                        {"window": "weekly", "remaining_fraction": 0.86, "description": "Refreshes in 5 days."},
                        {"window": "5h", "remaining_fraction": 0.47, "description": "Refreshes in 3 hours."}
                    ]
                }]
            }),
        );
        checkout.usage.insert(
            Provider::OpenCode,
            json!({
                "go": {
                    "rolling": {"percent": 0.0, "resetsAt": now + (4 * 60 + 59) * 60_000, "limited": false}
                }
            }),
        );
        let entries = collect_usage(&ws);
        let summary: Vec<(&str, &str, &str, u64)> = entries
            .iter()
            .map(|e| {
                (
                    e.label.as_str(),
                    e.sublabel.as_str(),
                    e.reset.as_str(),
                    e.percent.round() as u64,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Gemini", "5h", "3h", 53),
                ("Gemini", "7d", "5d", 14),
                ("OpenCode", "Go", "5h", 0),
            ]
        );
    }

    #[test]
    fn picks_most_constrained_bucket_per_group_when_multiple_groups_exist() {
        let ws = overview_state();
        let entries = collect_usage(&ws);
        let summary: Vec<(&str, &str, &str, u64)> = entries
            .iter()
            .map(|e| {
                (
                    e.label.as_str(),
                    e.sublabel.as_str(),
                    e.reset.as_str(),
                    e.percent.round() as u64,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("Gemini", "5h", "3h", 53),
                ("Claude", "5h", "2h", 45),
                ("OpenCode", "Go 7d", "2d", 41),
            ]
        );
    }

    #[test]
    fn opencode_only_shows_all_three_allowances() {
        let mut ws = overview_state();
        let now = live(&ws).feed.displayed_at();
        let checkout = live_mut(&mut ws)
            .app
            .services
            .checkouts
            .get_mut("/projects/rune")
            .unwrap();
        checkout.usage.remove(&Provider::Antigravity);
        checkout.usage.insert(
            Provider::OpenCode,
            json!({
                "go": {
                    "rolling": {"percent": 12.0, "resetsAt": now + 3 * 3_600_000, "limited": false},
                    "weekly": {"percent": 41.0, "resetsAt": now + 2 * 86_400_000, "limited": false},
                    "monthly": {"percent": 19.0, "resetsAt": now + 18 * 86_400_000, "limited": false}
                }
            }),
        );
        let entries = collect_usage(&ws);
        let summary: Vec<(&str, &str, &str, u64)> = entries
            .iter()
            .map(|e| {
                (
                    e.label.as_str(),
                    e.sublabel.as_str(),
                    e.reset.as_str(),
                    e.percent.round() as u64,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("OpenCode", "Rolling", "3h", 12),
                ("OpenCode", "Weekly", "2d", 41),
                ("OpenCode", "Monthly", "18d", 19),
            ]
        );
    }
}
