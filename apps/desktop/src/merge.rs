//! GitHub-style review readiness, expandable checks and a joined merge action.
use crate::{
    Workspace,
    live::{live, live_mut},
    select::Select,
    ui,
};
use caelum_core::application::{
    Task,
    presentation::{Check, CheckSummary, MergeReadiness, Outcome, now},
};
use caelum_protocol::{GitHubMergeMethod, GitHubPullRequestDetails};
use rune::{El, col, row, text};
use rune_kit::{ButtonVariant, Kit, SelectAppearance, glyphs};

fn status(title: &str, detail: &str, outcome: Outcome, k: &Kit) -> El {
    let tint = match outcome {
        Outcome::Failure => k.alpha(k.c.danger.fg, 0.055),
        Outcome::Interrupted => k.alpha(k.c.warning.fg, 0.055),
        _ => rune::Color::TRANSPARENT,
    };
    row()
        .w_full()
        .gap(10.0)
        .px(12.0)
        .py(9.0)
        .center_y()
        .fill(tint)
        .child(if outcome == Outcome::Running {
            rune::icon(&ui::CLOCK, 14.0).color(k.c.icon_muted)
        } else {
            ui::outcome(outcome, k)
        })
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(if outcome == Outcome::Running {
                    ui::busy_text(title, k.ty.ui_medium, k)
                } else {
                    text(title).font(k.ty.ui_medium).color(k.c.text_base).wrap()
                })
                .child(ui::caption(detail, k)),
        )
}

pub fn view(
    s: &Workspace,
    k: &Kit,
    project: &str,
    url: &str,
    details: &GitHubPullRequestDetails,
    layers: &mut Vec<El>,
) -> El {
    let l = live(s);
    if l.app
        .services
        .reviews
        .get(&(project.into(), url.into()))
        .is_some_and(|r| r.cached())
    {
        return status(
            "Cached review",
            "Checks and merge readiness will update when GitHub responds.",
            Outcome::Neutral,
            k,
        )
        .key("review-cached-readiness");
    }
    let owner = (project.to_owned(), url.to_owned());
    let open = l.panels.expanded_checks.contains(&owner);
    let summary = CheckSummary::new(details.status_check_rollup.as_deref());
    let (review, explanation, outcome) = match details.pull_request.review_decision.as_deref() {
        Some("APPROVED") => (
            "Review approved",
            "The latest review decision is approved.",
            Outcome::Success,
        ),
        Some("CHANGES_REQUESTED") => (
            "Changes requested",
            "Address the requested changes before merging.",
            Outcome::Failure,
        ),
        Some("REVIEW_REQUIRED") => (
            "Review required",
            "An approving review is required for this pull request.",
            Outcome::Interrupted,
        ),
        _ => (
            "No review decision reported",
            "Review requirements are determined by this repository.",
            Outcome::Neutral,
        ),
    };
    let mut body = col()
        .key("review-merge-box")
        .w_full()
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .child(status(review, explanation, outcome, k).key("review-approval"))
        .child(ui::divider(k));
    let checks = if let Some(list) = details
        .status_check_rollup
        .as_ref()
        .filter(|checks| !checks.is_empty())
    {
        let count = list.len();
        let count_label = if count == 1 {
            "1 check".to_owned()
        } else {
            format!("{count} checks")
        };
        let summary_row = status(summary.title, &summary.detail, summary.outcome, k).child(
            row()
                .gap(6.0)
                .center_y()
                .child(
                    text(count_label)
                        .font(k.ty.small_medium)
                        .color(k.c.text_muted),
                )
                .child(ui::disclosure(open, k)),
        );
        col().key("review-check-summary").w_full().child(
            k.interactive(summary_row)
                .key("review-checks-toggle")
                .role("button")
                .label("Check details")
                .help(if open { "Hide checks" } else { "Show checks" })
                .expanded(open)
                .focusable(true)
                .focus_ring(1.5, -1.5, k.c.border_focus)
                .on_press(move |s: &mut Workspace| {
                    let expanded = &mut live_mut(s).panels.expanded_checks;
                    if !expanded.remove(&owner) {
                        expanded.insert(owner.clone());
                    }
                }),
        )
    } else {
        status(summary.title, &summary.detail, summary.outcome, k).key("review-check-summary")
    };
    body = body.child(checks);
    if open {
        let mut reported: Vec<_> = details
            .status_check_rollup
            .iter()
            .flatten()
            .map(|c| Check::new(c, now()))
            .collect();
        reported.sort_by_key(|c| match c.outcome {
            Outcome::Failure | Outcome::Interrupted => 0,
            Outcome::Running => 1,
            Outcome::Success => 2,
            _ => 3,
        });
        let mut list = col().w_full().pb(8.0);
        for (i, check) in reported.into_iter().enumerate() {
            let check_url = check.url.map(str::to_owned);
            let check_name = check.name.to_owned();
            let failed = matches!(check.outcome, Outcome::Failure | Outcome::Interrupted);
            let existing_child = failed
                .then(|| {
                    l.app
                        .ci_fix_child_session(url, &details.head_ref_oid, &check_name)
                })
                .flatten();
            let pending_worktree = failed && l.app.ci_fix_pending(url, &check_name);
            let has_actions_logs = failed
                && check_url
                    .as_deref()
                    .is_some_and(|u| caelum_core::github::parse_actions_run(u).is_some());
            let loaded_logs = l
                .app
                .services
                .reviews
                .get(&(project.into(), url.into()))
                .and_then(|r| r.check_logs.get(&check_name))
                .cloned();
            let log_task = has_actions_logs.then(|| Task::CheckLogs {
                project: project.into(),
                url: url.into(),
                head: details.head_ref_oid.clone(),
                check_name: check_name.clone(),
                details_url: check_url.clone().unwrap_or_default(),
            });
            let loading_logs = log_task
                .as_ref()
                .is_some_and(|task| l.app.services.busy(task));
            let fix_action = if let Some(child_index) = existing_child {
                let child = &l.app.sessions[child_index];
                let status_label = if l.app.needs_input(child) {
                    "Fix · Needs input"
                } else if child.working() {
                    "Fix · Working"
                } else {
                    "Open fix session"
                };
                Some(
                    ui::Button::new(status_label)
                        .variant(ButtonVariant::Outline)
                        .on_press(move |s: &mut Workspace| {
                            let l = live_mut(s);
                            l.app.select(child_index);
                            l.sync_editors();
                            l.panels.screen = crate::panels::Screen::Conversation;
                            l.panels.set_overlay(None);
                        })
                        .build(k)
                        .h(24.0)
                        .px(8.0)
                        .key(format!("check-open-fix-{i}")),
                )
            } else if failed {
                let pr_project = project.to_owned();
                let pr_url = url.to_owned();
                let name = check_name.clone();
                let details_link = check_url.clone();
                Some(
                    ui::Button::new(if pending_worktree {
                        "Preparing worktree…"
                    } else {
                        "Fix in child session"
                    })
                    .variant(ButtonVariant::Outline)
                    .disabled(pending_worktree)
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        if l.app
                            .fix_failing_check(
                                pr_project.clone(),
                                pr_url.clone(),
                                name.clone(),
                                details_link.clone(),
                            )
                            .is_some()
                        {
                            l.sync_editors();
                            l.panels.screen = crate::panels::Screen::Conversation;
                            l.panels.set_overlay(None);
                        }
                    })
                    .build(k)
                    .h(24.0)
                    .px(8.0)
                    .key(format!("check-fix-{i}")),
                )
            } else {
                None
            };
            let log_action = log_task.map(|task| {
                ui::Button::new(if loading_logs {
                    "Loading…"
                } else if loaded_logs.is_some() {
                    "Refresh logs"
                } else {
                    "Logs"
                })
                .variant(ButtonVariant::GhostMuted)
                .disabled(loading_logs)
                .on_press(move |s: &mut Workspace| live_mut(s).app.task(task.clone()))
                .build(k)
                .h(24.0)
                .px(8.0)
                .key(format!("check-logs-{i}"))
            });
            let mut item = col().w_full().child(
                k.interactive(
                    row()
                        .w_full()
                        .gap(8.0)
                        .px(12.0)
                        .py(8.0)
                        .center_y()
                        .child(if check.outcome == Outcome::Running {
                            rune::icon(&ui::CLOCK, 14.0).color(k.c.icon_muted)
                        } else {
                            ui::outcome(check.outcome, k)
                        })
                        .child(
                            text(check.name)
                                .font(k.ty.small_medium)
                                .color(k.c.text_base)
                                .truncate()
                                .min_w(0.0)
                                .grow(1.0)
                                .key(format!("check-name-{i}")),
                        )
                        .children(log_action)
                        .children(fix_action)
                        .child(if check.outcome == Outcome::Running {
                            ui::busy_text("Running", k.ty.small, k)
                                .w(62.0)
                                .help(check.label)
                        } else {
                            ui::caption(
                                match check.outcome {
                                    Outcome::Success => "Passed",
                                    Outcome::Failure => "Failed",
                                    Outcome::Running => "Running",
                                    Outcome::Interrupted => "Cancelled",
                                    Outcome::Neutral => "Skipped",
                                    Outcome::Unknown => "Unknown",
                                },
                                k,
                            )
                            .help(check.label)
                            .w(62.0)
                        })
                        .child(ui::caption(check.timing, k).w(60.0))
                        .child(
                            rune::icon(&ui::EXTERNAL, 12.0)
                                .color(k.c.icon_muted)
                                .opacity(if check_url.is_some() { 1.0 } else { 0.0 }),
                        ),
                )
                .key(format!("check-{i}"))
                .role("link")
                .label(check.name)
                .focusable(check_url.is_some())
                .focus_ring(1.0, 0.0, k.c.border_focus)
                .on_press_tx(move |_: &mut Workspace, tx| {
                    if let Some(url) = &check_url {
                        let _ = tx.open_url(url);
                    }
                }),
            );
            if let Some(logs) = loaded_logs {
                let copy_logs = logs.excerpt.clone();
                item = item.child(
                    col()
                        .w_full()
                        .px(12.0)
                        .pb(8.0)
                        .gap(4.0)
                        .key(format!("check-log-preview-{i}"))
                        .child(
                            row()
                                .w_full()
                                .center_y()
                                .child(
                                    ui::caption(
                                        if logs.truncated {
                                            "Failure log (tail)"
                                        } else {
                                            "Failure log"
                                        },
                                        k,
                                    )
                                    .grow(1.0),
                                )
                                .child(
                                    ui::Button::new("Copy logs")
                                        .variant(ButtonVariant::GhostMuted)
                                        .on_press_tx(move |_: &mut Workspace, tx| {
                                            tx.copy(copy_logs.clone())
                                        })
                                        .build(k)
                                        .h(22.0)
                                        .px(6.0),
                                ),
                        )
                        .child(
                            col()
                                .w_full()
                                .max_h(180.0)
                                .padding(8.0)
                                .radius(6.0)
                                .fill(k.c.layer_02)
                                .scroll(rune::ScrollSpec { stick_bottom: true })
                                .child(
                                    text(&logs.excerpt)
                                        .font(k.ty.mono)
                                        .color(k.c.text_base)
                                        .selectable(true)
                                        .wrap_anywhere(),
                                ),
                        ),
                );
            }
            list = list.child(item);
        }
        body = body.child(list);
    }
    let readiness = MergeReadiness::new(details);
    let mut branch =
        status(readiness.title, readiness.detail, readiness.outcome, k).key("review-branch");
    if readiness.behind {
        let task = Task::UpdateBranch {
            project: project.into(),
            url: url.into(),
            head: details.head_ref_oid.clone(),
        };
        let busy = l.app.services.busy(&task);
        branch = branch.child(
            ui::Button::new(if busy { "Updating…" } else { "Update branch" })
                .disabled(busy)
                .on_press(move |s: &mut Workspace| live_mut(s).app.task(task.clone()))
                .build(k)
                .key("update-review-branch"),
        );
    }
    body = body.child(ui::divider(k)).child(branch);
    if details.state != "OPEN" || details.pull_request.is_draft {
        return body;
    }

    let methods = [
        ("Squash and merge", GitHubMergeMethod::Squash),
        ("Create merge commit", GitHubMergeMethod::Merge),
        ("Rebase and merge", GitHubMergeMethod::Rebase),
    ];
    let (name, method) = methods[l.panels.merge_method];
    let task = Task::Merge {
        project: project.into(),
        url: url.into(),
        head: details.head_ref_oid.clone(),
        method,
    };
    let busy = l.app.services.busy(&task);
    let ready = readiness.can_merge;
    let green = rune::color::hex(if k.dark { "#238636" } else { "#1f883d" }).unwrap();
    let mut merge_kit = k.clone();
    if ready {
        merge_kit.c.bg_accent = green;
        merge_kit.c.submit_fg = rune::Color::WHITE;
        merge_kit.c.icon_muted = rune::Color::WHITE;
    } else {
        merge_kit.c.icon_muted = k.c.icon_base;
    }
    let selector_fill = if ready {
        if l.panels.merge_menu {
            k.alpha(rune::Color::WHITE, 0.14)
        } else {
            rune::Color::TRANSPARENT
        }
    } else if l.panels.merge_menu {
        k.c.hover
    } else {
        rune::Color::TRANSPARENT
    };
    let names: Vec<_> = methods.iter().map(|m| m.0).collect();
    let selector = Select::new("merge-method", &names)
        .current(Some(name))
        .display("")
        .label("Merge method")
        .appearance(SelectAppearance::Field)
        .width(34.0)
        .indicator(&glyphs::CHEVRON_DOWN)
        .disabled(busy)
        .open(l.panels.merge_menu)
        .on_toggle(|s: &mut Workspace| live_mut(s).panels.merge_menu = !live(s).panels.merge_menu)
        .on_close(|s: &mut Workspace| live_mut(s).panels.merge_menu = false)
        .on_choose(move |s: &mut Workspace, value, _| {
            let p = &mut live_mut(s).panels;
            p.merge_method = methods.iter().position(|m| m.0 == value).unwrap_or(0);
            p.merge_menu = false;
        })
        .build(&merge_kit, layers)
        .h(34.0)
        .radius(0.0)
        .fill(selector_fill)
        .border(0.0, rune::Color::TRANSPARENT);
    let auto_merge = Task::SetAutoMerge {
        project: project.into(),
        url: url.into(),
        head: details.head_ref_oid.clone(),
        method: details.auto_merge_request.is_none().then_some(method),
    };
    let auto_label = if details.auto_merge_request.is_some() {
        "Disable auto-merge"
    } else {
        "Enable auto-merge"
    };
    let merge_action = ui::Button::new(if busy { "Working…" } else { name })
        .variant(if ready {
            ButtonVariant::Contrast
        } else {
            ButtonVariant::Neutral
        })
        .disabled(busy || !ready)
        .on_press(move |s: &mut Workspace| live_mut(s).app.task(task.clone()))
        .build(&merge_kit)
        .key("merge-pull-request")
        .h(34.0)
        .radius(0.0)
        .border(0.0, rune::Color::TRANSPARENT);
    let mut split = row()
        .key("merge-controls")
        .h(34.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(if ready { green } else { k.c.layer_02 })
        .child(merge_action)
        .child(rune::el().w(1.0).h_full().fill(if ready {
            k.alpha(rune::Color::WHITE, 0.25)
        } else {
            k.c.border_strong
        }))
        .child(selector);
    if !ready {
        split = split.border(1.0, k.c.border_strong);
    }
    let mut controls = col().w_full().px(12.0).py(10.0).gap(8.0).child(
        row()
            .w_full()
            .flex_wrap()
            .gap(8.0)
            .center_y()
            .child(split)
            .child(
            ui::Button::new(if busy { "Working…" } else { auto_label })
                .disabled(busy)
                .on_press(move |s: &mut Workspace| live_mut(s).app.task(auto_merge.clone()))
                .build(k)
                .key("set-auto-merge")
                .label(auto_label)
                .help(
                    "Auto-merge uses the selected merge method once GitHub's requirements are met.",
                )
                .h(34.0),
        ),
    );
    if let Some(request) = &details.auto_merge_request {
        let method = match request.merge_method {
            GitHubMergeMethod::Merge => "merge commit",
            GitHubMergeMethod::Squash => "squash",
            GitHubMergeMethod::Rebase => "rebase",
        };
        controls = controls.child(
            ui::caption(
                format!("Auto-merge enabled · {method}. Waiting for GitHub's requirements."),
                k,
            )
            .key("auto-merge-status"),
        );
    }
    body.child(ui::divider(k)).child(controls)
}
