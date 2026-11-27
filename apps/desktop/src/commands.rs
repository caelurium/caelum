//! Native surfaces for local commands; provider requests and results stay in core.
mod controls;
mod dispatch;
mod fork;
mod sessions;
mod timeline;
mod usage;

pub(crate) use controls::{export, history, rename};
pub use dispatch::dispatch;
pub use fork::fork_key;
pub(crate) use fork::header as history_header;
use sessions::sessions;
pub use sessions::sessions_key;
pub(crate) use timeline::open as open_timeline;
use timeline::timeline;
pub use timeline::timeline_key;

use crate::context::EditMenu;
use crate::ui::Button;
use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    quiet_button, theme, ui,
};
use caelum_core::application::{Provider, Scope, Task};
use caelum_protocol::{OpenCodeStatsFilter, StatsProvider};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, row, text, text_input};
use rune_kit::{ButtonVariant, Kit, Markdown, ScrollBar, glyphs};

pub struct State {
    rename: Option<controls::RenameForm>,
    fork_selected: Option<caelum_core::application::ForkBoundary>,
    pub owner: Option<usize>,
    pub usage_target: Option<Scope>,
    pub search: TextEdit,
    pub stats_project: Option<String>,
    pub stats_provider: Option<StatsProvider>,
    stats_target: Option<String>,
    pub stats_days: Option<u64>,
    pub stats_anchor: u64,
    pub stats_projects_open: bool,
    pub timeline_selected: Option<String>,
    pub timeline_current: Option<String>,
    pub timeline_bookmarked_only: bool,
    pub sessions_search: TextEdit,
    pub sessions_selected: Option<String>,
    pub side_owner: Option<usize>,
    pub side_editor: TextEdit,
    pub previous_drafts: Vec<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            rename: None,
            fork_selected: None,
            owner: None,
            usage_target: None,
            search: TextEdit::new(""),
            stats_project: None,
            stats_provider: None,
            stats_target: None,
            stats_days: None,
            stats_anchor: 0,
            stats_projects_open: false,
            timeline_selected: None,
            timeline_current: None,
            timeline_bookmarked_only: false,
            sessions_search: TextEdit::new(""),
            sessions_selected: None,
            side_owner: None,
            side_editor: TextEdit::new(""),
            previous_drafts: Vec::new(),
        }
    }
}

pub fn close_side(s: &mut Workspace) {
    let l = live_mut(s);
    if let Some(index) = l.commands.side_owner.take() {
        l.app.dismiss_side_question(index);
    }
    l.focus();
}
pub fn side_key(s: &mut Workspace, ev: &KeyEvent, _: &mut Tx) -> bool {
    if ev.key == Key::Escape && live(s).commands.side_owner.is_some() {
        close_side(s);
        true
    } else {
        false
    }
}
pub fn ask(s: &mut Workspace, _: &mut Tx) {
    let l = live_mut(s);
    let Some(index) = l.commands.side_owner else {
        return;
    };
    if l.app.sessions[index].attaching == 0 && l.app.ask(index, l.commands.side_editor.text.clone())
    {
        l.commands.side_editor.focused = false;
    }
}
pub fn side_view(s: &Workspace, k: &Kit, padding: f32) -> El {
    let l = live(s);
    let index = l.commands.side_owner.unwrap();
    let session = &l.app.sessions[index];
    let mut body = col()
        .w_full()
        .max_w(840.0)
        .gap(8.0)
        .padding(12.0)
        .radius(10.0)
        .border(1.0, k.c.border_base)
        .fill(k.c.bg_base)
        .child(
            row()
                .w_full()
                .center_y()
                .child(
                    text("Side question")
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .grow(1.0),
                )
                .child(
                    quiet_button(&glyphs::CLOSE, "Close side question")
                        .on_press(close_side)
                        .build(k),
                ),
        );
    for attachment in &session.side_attachments {
        let id = attachment.id.clone();
        body = body.child(
            Button::new(format!("{} ×", attachment.label()))
                .variant(ButtonVariant::GhostMuted)
                .disabled(session.generating_side_answer())
                .on_press(move |s: &mut Workspace| {
                    live_mut(s).app.sessions[index]
                        .side_attachments
                        .retain(|a| a.id != id)
                })
                .build(k),
        );
    }
    if let Some((question, answer)) = session.side_exchange() {
        body = body.child(ui::caption(
            caelum_core::application::presentation::visible_text(question),
            k,
        ));
        if session.generating_side_answer() {
            body = body.child(ui::caption("Answering independently…", k).role("status"));
        }
        if !answer.is_empty() {
            body = body.child(
                col()
                    .key("side-answer")
                    .w_full()
                    .max_h(150.0)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        Markdown::new(caelum_core::application::presentation::visible_text(answer))
                            .selectable(true)
                            .render(&theme::markdown(k)),
                    ),
            );
        }
        if let Some(error) = session.side_error() {
            body = body.child(ui::caption(error, k).color(k.c.danger.fg));
        }
        if !session.generating_side_answer() {
            let copy = caelum_core::application::presentation::visible_text(answer);
            let mut actions = row().gap(8.0).child(
                Button::new("Edit question")
                    .variant(ButtonVariant::Outline)
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        l.app.sessions[index].edit_side_question();
                        l.commands.side_editor.focused = true;
                    })
                    .build(k)
                    .key("edit-side-question"),
            );
            if session.side_error().is_some() {
                actions = actions.child(
                    Button::new("Retry")
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(ask)
                        .build(k)
                        .key("retry-side-question"),
                );
            } else {
                actions = actions.child(
                    Button::new("Copy answer")
                        .icon(&glyphs::COPY)
                        .variant(ButtonVariant::Contrast)
                        .on_press_tx(move |_: &mut Workspace, tx| tx.copy(copy.clone()))
                        .build(k),
                );
            }
            body = body
                .child(ui::divider(k))
                .child(actions.w_full().justify_end());
        }
    } else {
        body = body
            .child(crate::completion::view(s, k))
            .children(
                session
                    .side_error()
                    .map(|error| ui::caption(error, k).color(k.c.danger.fg).wrap()),
            )
            .child(rune::auto_grow(
                text_input(&l.commands.side_editor, k.ty.ui)
                    .label("Question")
                    .placeholder("Ask about this session…", k.c.text_muted)
                    .color(k.c.text_base)
                    .w_full()
                    .edit_menu_changed(
                        "side-question-input",
                        |s| Some(&mut live_mut(s).commands.side_editor),
                        crate::completion::changed,
                    )
                    .on_input_key(crate::on_key)
                    .on_submit(ask),
                2,
                4,
            ))
            .child(ui::divider(k))
            .child(
                row()
                    .w_full()
                    .center_y()
                    .justify_between()
                    .gap(8.0)
                    .child(ui::caption("Enter to ask · Esc to close", k))
                    .child(
                        Button::new("Ask")
                            .variant(ButtonVariant::Contrast)
                            .disabled(
                                l.commands.side_editor.text.trim().is_empty()
                                    || session.attaching > 0,
                            )
                            .on_press_tx(ask)
                            .build(k),
                    ),
            );
    }
    if let Some(activity) = crate::activity::session_activity(s, index) {
        body = body.child(crate::activity::indicator(
            &activity,
            session.options.agent.as_deref(),
            k,
        ));
    }
    col().w_full().items_center().px(padding).pb(20.0).child(
        body.role("region")
            .label("Side question")
            .key("side-question")
            .focusable(true),
    )
}

pub(crate) fn open_usage(s: &mut Workspace, scope: Scope) {
    let l = live_mut(s);
    l.commands.usage_target = Some(scope.clone());
    l.panels.set_overlay(Some(Screen::Usage));
    let task = Task::Usage {
        project: scope.project,
        provider: scope.provider,
    };
    if !l.fixture && !l.app.services.busy(&task) {
        l.app.task(task);
    }
}

pub(crate) fn usage_provider(s: &Workspace) -> Provider {
    let l = live(s);
    if let Some(scope) = &l.commands.usage_target {
        return scope.provider;
    }
    l.commands
        .owner
        .or(l.app.selected)
        .and_then(|i| l.app.sessions.get(i))
        .map(|session| session.scope.provider)
        .unwrap_or(Provider::OpenCode)
}

pub fn view(s: &Workspace, k: &Kit, screen: Screen, layers: &mut Vec<El>) -> El {
    if screen == Screen::Usage {
        return usage::view(s, k);
    }
    if screen == Screen::Stats {
        return stats(s, k, layers);
    }
    let Some(index) = live(s).commands.owner else {
        return col();
    };
    match screen {
        Screen::Rename => controls::rename_view(s, k),
        Screen::Fork => fork::view(s, k),
        Screen::Timeline => timeline(s, k, index),
        Screen::Sessions => sessions(s, k, index),
        _ => col(),
    }
}

pub(crate) fn open_stats(s: &mut Workspace) {
    let l = live_mut(s);
    let local = l
        .app
        .active_project
        .as_ref()
        .filter(|p| caelum_core::remote::native::checkout_id(p).is_none())
        .or_else(|| {
            l.app
                .projects
                .iter()
                .find(|p| caelum_core::remote::native::checkout_id(&p.path).is_none())
                .map(|p| &p.path)
        });
    l.commands.stats_target = local
        .cloned()
        .or_else(|| {
            l.app
                .selected
                .map(|i| l.app.sessions[i].scope.project.clone())
        })
        .or_else(|| {
            l.app
                .remote
                .saved
                .selected
                .as_deref()
                .map(caelum_core::remote::native::project_key)
        });
    l.commands.stats_anchor = caelum_core::application::presentation::now();
    l.commands.stats_projects_open = false;
    l.commands.search.clear();
    l.panels.set_overlay(Some(Screen::Stats));
    refresh_stats(s);
}
fn stats_owner(s: &Workspace) -> Option<&str> {
    let l = live(s);
    l.commands.stats_target.as_deref().or_else(|| {
        l.commands
            .owner
            .map(|i| l.app.sessions[i].scope.project.as_str())
    })
}
fn stats_task(s: &Workspace) -> Option<Task> {
    let l = live(s);
    Some(Task::Stats {
        project: stats_owner(s)?.into(),
        filter: OpenCodeStatsFilter {
            from: l
                .commands
                .stats_days
                .map(|days| l.commands.stats_anchor.saturating_sub(days * 86_400_000)),
            project: l.commands.stats_project.clone(),
            provider: l.commands.stats_provider,
            ..Default::default()
        },
    })
}
pub(crate) fn refresh_stats(s: &mut Workspace) {
    if let Some(task) = stats_task(s) {
        live_mut(s).app.task(task);
    }
}
fn stats(s: &Workspace, k: &Kit, layers: &mut Vec<El>) -> El {
    let l = live(s);
    let Some(owner) = stats_owner(s) else {
        return col();
    };
    let checkout = l.app.services.checkouts.get(owner);
    let mut track = ui::segmented_track(k);
    for (name, days) in [
        ("All history", None),
        ("7 days", Some(7)),
        ("30 days", Some(30)),
    ] {
        track = track.child(
            ui::segment(name, l.commands.stats_days == days, k).on_press(
                move |s: &mut Workspace| {
                    live_mut(s).commands.stats_days = days;
                    refresh_stats(s);
                },
            ),
        );
    }
    let ranges = row().w_full().gap(8.0).center_y().child(track);
    let project_name = checkout
        .and_then(|c| {
            c.stats_projects
                .iter()
                .find(|p| Some(&p.id) == l.commands.stats_project.as_ref())
        })
        .and_then(|p| p.name.as_deref())
        .or(l.commands.stats_project.as_deref())
        .unwrap_or("All projects");
    let query = l.commands.search.text.to_lowercase();
    let mut projects = vec![("All projects".to_owned(), None)];
    if let Some(c) = checkout {
        projects.extend(
            c.stats_projects
                .iter()
                .filter(|p| {
                    format!("{} {}", p.name.as_deref().unwrap_or_default(), p.canonical)
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|p| {
                    (
                        format!(
                            "{} · {}",
                            p.name.as_deref().unwrap_or("Project"),
                            p.canonical
                        ),
                        Some(p.id.clone()),
                    )
                }),
        );
    }
    let options: Vec<_> = projects.iter().map(|p| p.0.clone()).collect();
    let current = projects
        .iter()
        .find(|p| p.1 == l.commands.stats_project)
        .map(|p| p.0.as_str());
    let ranges = ranges.child(rune::el().grow(1.0)).child(
        crate::select::Select::new("stats-project", &options)
            .current(current)
            .display(project_name)
            .label("Statistics project")
            .appearance(rune_kit::SelectAppearance::Field)
            .width(180.0)
            .open(l.commands.stats_projects_open)
            .on_toggle(|s: &mut Workspace| {
                let c = &mut live_mut(s).commands;
                c.stats_projects_open = !c.stats_projects_open;
                c.search.clear();
                c.search.focused = c.stats_projects_open;
            })
            .on_close(|s: &mut Workspace| {
                live_mut(s).commands.stats_projects_open = false;
                live_mut(s).commands.search.focused = false;
            })
            .on_choose(move |s: &mut Workspace, value, _| {
                let c = &mut live_mut(s).commands;
                c.stats_project = projects
                    .iter()
                    .find(|p| p.0 == value)
                    .and_then(|p| p.1.clone());
                c.stats_projects_open = false;
                c.search.focused = false;
                refresh_stats(s);
            })
            .search(
                ui::field(
                    &l.commands.search,
                    "Find statistics project",
                    "Search projects…",
                    k,
                )
                .edit_menu("stats-project-search", |s| {
                    Some(&mut live_mut(s).commands.search)
                }),
            )
            .build(k, layers),
    );
    let mut providers = ui::segmented_track(k);
    for choice in [
        None,
        Some(StatsProvider::Codex),
        Some(StatsProvider::OpenCode),
        Some(StatsProvider::Antigravity),
        Some(StatsProvider::Boat),
    ] {
        let name = choice.map_or("All", StatsProvider::name);
        providers = providers.child(
            ui::segment(name, l.commands.stats_provider == choice, k)
                .key(format!("stats-provider-{name}"))
                .on_press(move |s: &mut Workspace| {
                    live_mut(s).commands.stats_provider = choice;
                    refresh_stats(s);
                }),
        );
    }
    let mut body = col()
        .w_full()
        .gap(12.0)
        .child(ranges)
        .child(row().child(providers).label("Statistics provider"));
    if let Some(stats) = checkout.and_then(|c| c.stats.as_ref()) {
        let mut totals = row().w_full().gap(8.0);
        for (name, value) in [
            ("Sessions", stats.sessions.to_string()),
            ("Prompts", stats.prompts.to_string()),
            (
                "Observed time",
                caelum_core::application::presentation::duration(stats.duration_ms),
            ),
            ("Est. cost", cost(stats.cost, stats.unpriced, stats.prompts)),
        ] {
            totals = totals.child(
                ui::card(k)
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(8.0)
                    .child(ui::caption(name, k))
                    .child(
                        text(value)
                            .font(rune::TextStyle::new(26.0, 550.0))
                            .color(k.c.text_base),
                    ),
            );
        }
        body = body.child(totals);
        if stats.recorded_only {
            body=body.child(ui::caption("OpenCode server history unavailable for this scope · Showing locally recorded turns.",k));
        }
        if stats.estimated_turns > 0 || stats.unpriced > 0 {
            body = body.child(ui::caption(
                format!(
                    "{} turns with approximate text tokens · {} turns without a price",
                    stats.estimated_turns, stats.unpriced
                ),
                k,
            ));
        }
        if stats.dropped > 0 {
            body = body.child(ui::caption(
                format!("Local storage missed {} usage samples.", stats.dropped),
                k,
            ));
        }
        let mut activity = ui::card(k).gap(12.0).child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(text("Activity").font(k.ty.ui_medium).color(k.c.text_base))
                .child(ui::caption(
                    format!(
                        "{} active days · {} day streak",
                        stats.active_days, stats.streak
                    ),
                    k,
                )),
        );
        if stats.activity.is_empty() {
            activity = activity.child(
                ui::caption("No activity in this period. Try a wider date range.", k).py(20.0),
            );
        } else {
            let days = &stats.activity[stats.activity.len().saturating_sub(60)..];
            let peak = days.iter().map(|d| d.steps).max().unwrap_or(1).max(1) as f32;
            let mut bars = row().w_full().h(92.0).gap(3.0);
            for day in days {
                let description = format!("{} · {} steps", day.date, day.steps);
                bars = bars.child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .h_full()
                        .justify_end()
                        .child(
                            rune::el()
                                .w_full()
                                .h((day.steps as f32 / peak * 88.0).max(3.0))
                                .radius(3.0)
                                .fill(k.alpha(
                                    k.c.text_accent,
                                    if day.steps == 0 { 0.12 } else { 0.65 },
                                )),
                        )
                        .role("img")
                        .label(&description)
                        .help(description)
                        .focusable(true),
                );
            }
            activity = activity.child(bars).child(
                row()
                    .w_full()
                    .justify_between()
                    .child(ui::caption(&days[0].date, k))
                    .child(ui::caption(format!("{} steps", stats.steps), k))
                    .child(ui::caption(&days[days.len() - 1].date, k)),
            );
        }
        body = body.child(activity);
        let mut tokens = ui::card(k).gap(10.0).child(
            text("Token breakdown")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
        let total = (stats.tokens.input
            + stats.tokens.output
            + stats.tokens.reasoning
            + stats.tokens.cache.read
            + stats.tokens.cache.write)
            .max(1.0);
        for (name, n) in [
            ("Input", stats.tokens.input),
            ("Output", stats.tokens.output),
            ("Reasoning", stats.tokens.reasoning),
            ("Cache read", stats.tokens.cache.read),
            ("Cache write", stats.tokens.cache.write),
        ] {
            tokens = tokens.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(ui::caption(name, k).w(82.0))
                    .child(
                        row().grow(1.0).h(5.0).radius(3.0).fill(k.c.layer_03).child(
                            rune::el()
                                .w_frac((n / total).clamp(0.0, 1.0) as f32)
                                .h_full()
                                .radius(3.0)
                                .fill(k.alpha(k.c.text_accent, 0.65)),
                        ),
                    )
                    .child(
                        row()
                            .w(68.0)
                            .justify_end()
                            .child(text(compact(n)).font(k.ty.mono).color(k.c.text_base)),
                    )
                    .help(format!("{name}: {n:.0} tokens")),
            );
        }
        body = body.child(tokens);
        let mut models = ui::card(k).gap(10.0).child(
            row()
                .w_full()
                .gap(16.0)
                .child(
                    text("Models")
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .grow(1.0),
                )
                .child(row().w(54.0).justify_end().child(ui::caption("Steps", k)))
                .child(row().w(60.0).justify_end().child(ui::caption("Cost", k))),
        );
        for model in &stats.models {
            models = models.child(ui::divider(k)).child(
                row()
                    .w_full()
                    .gap(16.0)
                    .center_y()
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .gap(3.0)
                            .child(
                                text(&model.model.id)
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate(),
                            )
                            .child(ui::caption(&model.model.provider_id, k)),
                    )
                    .child(
                        row().w(54.0).justify_end().child(
                            text(model.steps.to_string())
                                .font(k.ty.mono)
                                .color(k.c.text_muted),
                        ),
                    )
                    .child(
                        row().w(60.0).justify_end().child(
                            text(cost(model.cost, model.unpriced, model.steps))
                                .font(k.ty.mono)
                                .color(k.c.text_base),
                        ),
                    ),
            );
        }
        if stats.models.is_empty() {
            models = models.child(ui::caption("No model usage reported in this period.", k));
        }
        body = body.child(models);
    }
    if checkout.is_none_or(|c| c.stats.is_none()) && l.app.services.busy(&stats_task(s).unwrap()) {
        body = body
            .child(ui::card(k).child(ui::skeleton("Loading summary", 2, k)))
            .child(ui::card(k).child(ui::skeleton("Loading activity", 3, k)));
    } else if checkout.is_none_or(|c| c.stats.is_none())
        && l.app.services.error(&stats_task(s).unwrap()).is_none()
    {
        body = body.child(ui::empty(
            "No statistics yet",
            "Recorded sessions, tokens and model usage will appear here.",
            k,
        ));
    }
    service_status(
        body,
        s,
        k,
        stats_task(s).unwrap(),
        checkout.is_some_and(|c| c.stats.is_some()),
    )
}
fn cost(usd: f64, unknown: u64, count: u64) -> String {
    if unknown > 0 && unknown == count {
        "—".into()
    } else if unknown > 0 {
        format!("${usd:.2}+")
    } else {
        format!("${usd:.2}")
    }
}
fn compact(n: f64) -> String {
    if n >= 1e9 {
        format!("{:.1}B", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1}M", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.1}K", n / 1e3)
    } else {
        format!("{n:.0}")
    }
}
pub(super) fn service_status(mut body: El, s: &Workspace, k: &Kit, task: Task, cached: bool) -> El {
    let services = &live(s).app.services;
    if let Some(error) = services.error(&task) {
        body = body.child(ui::notice(
            "Couldn’t load results",
            &format!(
                "{error}{}",
                if cached {
                    " · Showing the last reported result."
                } else {
                    ""
                }
            ),
            k,
        ));
    }
    body = body.child(ui::divider(k)).child(
        row().w_full().justify_end().child(
            Button::new(if services.busy(&task) {
                "Loading…"
            } else if services.error(&task).is_some() {
                "Retry"
            } else {
                "Refresh"
            })
            .icon(&ui::REFRESH)
            .variant(ButtonVariant::Outline)
            .disabled(services.busy(&task))
            .on_press(move |s: &mut Workspace| live_mut(s).app.task(task.clone()))
            .build(k),
        ),
    );
    col()
        .key("command-results")
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(body)
        .scroll_bar(k)
}
