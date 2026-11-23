use caelum_core::application::SessionBucket;
use rune::{El, Icon, ScrollSpec, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, ScrollBar, glyphs};

use super::{
    RUNNING, SETTLED, Tab, WAITING, cards, next_waiting, on_key, pull_requests, refresh_all,
    sessions, worktrees,
};
use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    quiet_button, ui,
};

pub fn view(s: &Workspace, k: &Kit, _layers: &mut Vec<El>) -> El {
    let l = live(s);
    l.overview.frame.borrow_mut().reset();
    if let Some(owner) = l.overview.pending_bookmark.get()
        && let Some(mark) = l.reading_bookmark_for(owner)
    {
        l.overview.pending_bookmark.set(None);
        *l.overview.captured_bookmark.borrow_mut() = Some(mark);
    }
    let pane = rune::memo::window_layout("main-pane").unwrap_or([0.0, 0.0, 980.0, 900.0]);
    let pane_width = pane[2];
    let compact = pane_width < 700.0 || pane[3] < 680.0;
    let all_sessions = l.app.dashboard_sessions("", false);
    let (mut waiting, mut running, mut settled) = (0usize, 0usize, 0usize);
    for id in &all_sessions {
        match l.app.session_bucket(id) {
            SessionBucket::Waiting => waiting += 1,
            SessionBucket::Running => running += 1,
            SessionBucket::Settled => settled += 1,
        }
    }
    let pr_count = l.app.open_pull_requests().len();
    let worktree_count = worktrees::count(l);
    let header = row()
        .w_full()
        .center_y()
        .flex_wrap()
        .gap(12.0)
        .child(
            text("Overview")
                .font(TextStyle::new(if compact { 19.0 } else { 22.0 }, 580.0))
                .color(k.c.text_base)
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            row()
                .center_y()
                .gap(14.0)
                .child(status_counter(
                    &WAITING,
                    format!("{waiting} waiting"),
                    if waiting > 0 {
                        k.c.warning.fg
                    } else {
                        k.c.text_muted
                    },
                    "overview-requests",
                    "Highlight the next session waiting for you",
                    k,
                ))
                .child(status_badge(
                    &RUNNING,
                    format!("{running} running"),
                    if running > 0 {
                        k.c.text_accent
                    } else {
                        k.c.text_muted
                    },
                    k,
                ))
                .child(status_badge(
                    &SETTLED,
                    format!("{settled} settled"),
                    k.c.success.fg,
                    k,
                )),
        );
    let tabs = tab_bar(s, all_sessions.len(), pr_count, worktree_count, compact, k);
    let body = match l.overview.tab {
        Tab::Sessions => sessions::view(s, compact, k),
        Tab::PullRequests => pull_requests::view(s, k),
        Tab::Worktrees => worktrees::view(s, k),
    };
    col()
        .key("conversation-overview")
        .role("region")
        .label("Overview")
        .w_full()
        .h_full()
        .min_h(0.0)
        .items_center()
        .child(
            col()
                .w_full()
                .max_w(1120.0)
                .h_full()
                .min_h(0.0)
                .px(if compact { 16.0 } else { 28.0 })
                .pt(if compact { 14.0 } else { 24.0 })
                .pb(if compact { 10.0 } else { 18.0 })
                .gap(if compact { 12.0 } else { 18.0 })
                .child(header)
                .child(cards::view(s, pane_width, compact, k))
                .child(tabs)
                .child(
                    col()
                        .key("overview-scroll")
                        .w_full()
                        .grow(1.0)
                        .min_h(120.0)
                        .scroll(ScrollSpec {
                            stick_bottom: false,
                        })
                        .child(body)
                        .scroll_bar(k),
                ),
        )
}

fn status_counter(
    glyph: &'static Icon,
    label: String,
    tone: rune::Color,
    key: &'static str,
    help: &'static str,
    k: &Kit,
) -> El {
    k.interactive(
        row()
            .center_y()
            .gap(5.0)
            .h(26.0)
            .px(7.0)
            .radius(6.0)
            .child(icon(glyph, 13.0).color(tone).a11y_hidden())
            .child(text(&label).font(k.ty.small_medium).color(k.c.text_muted)),
    )
    .key(key)
    .role("button")
    .label(label)
    .help(help)
    .focusable(true)
    .focus_ring(1.5, 0.0, k.c.border_focus)
    .on_press_tx(next_waiting)
}

fn status_badge(glyph: &'static Icon, label: String, tone: rune::Color, k: &Kit) -> El {
    row()
        .center_y()
        .gap(5.0)
        .h(26.0)
        .child(icon(glyph, 13.0).color(tone).a11y_hidden())
        .child(text(label).font(k.ty.small_medium).color(k.c.text_muted))
}

fn tab_pill(
    title: &str,
    count: Option<usize>,
    active: bool,
    tab: Tab,
    key: &'static str,
    compact: bool,
    k: &Kit,
) -> El {
    k.interactive(
        row()
            .center_y()
            .gap(5.0)
            .h(30.0)
            .px(if compact { 9.0 } else { 12.0 })
            .radius(15.0)
            .fill(if active {
                k.c.layer_02
            } else {
                rune::Color::TRANSPARENT
            })
            .border(
                1.0,
                if active {
                    k.c.border_strong
                } else {
                    k.c.border_base
                },
            )
            .child(
                text(title)
                    .font(if active { k.ty.ui_medium } else { k.ty.ui })
                    .color(if active {
                        k.c.text_base
                    } else {
                        k.c.text_muted
                    }),
            )
            .children(count.map(|n| {
                text(n.to_string()).font(k.ty.small).color(if active {
                    k.c.text_muted
                } else {
                    k.c.text_faint
                })
            })),
    )
    .key(key)
    .role("tab")
    .label(title)
    .focusable(true)
    .focus_selected(active)
    .focus_ring(1.5, 0.0, k.c.border_focus)
    .on_press(move |s: &mut Workspace| {
        let l = live_mut(s);
        l.overview.tab = tab;
        l.overview.cursor = None;
    })
}

fn tab_bar(
    s: &Workspace,
    session_count: usize,
    pr_count: usize,
    worktree_count: usize,
    compact: bool,
    k: &Kit,
) -> El {
    let l = live(s);
    let placeholder = match l.overview.tab {
        Tab::Sessions => "Filter sessions…",
        Tab::PullRequests => "Filter pull requests…",
        Tab::Worktrees => "Filter worktrees…",
    };
    let search = row()
        .h(30.0)
        .w(if compact { 128.0 } else { 220.0 })
        .px(8.0)
        .gap(5.0)
        .center_y()
        .radius(8.0)
        .fill(k.c.bg_base)
        .border(
            1.0,
            if l.overview.search.focused {
                k.c.border_focus
            } else {
                k.c.border_base
            },
        )
        .child(
            icon(&glyphs::SEARCH, 12.0)
                .color(k.c.icon_muted)
                .a11y_hidden(),
        )
        .child(
            rune::text_input(&l.overview.search, k.ty.small)
                .single_line()
                .label("Filter overview")
                .placeholder(placeholder, k.c.text_muted)
                .color(k.c.text_base)
                .selection_color(k.selection())
                .grow(1.0)
                .min_w(0.0)
                .h(28.0)
                .py(5.0)
                .edit_menu_changed(
                    "overview-search",
                    |s| Some(&mut live_mut(s).overview.search),
                    |s, _| {
                        live_mut(s).overview.cursor = None;
                    },
                )
                .on_input_key(on_key),
        );
    row()
        .w_full()
        .center_y()
        .flex_wrap()
        .gap(if compact { 5.0 } else { 8.0 })
        .child(tab_pill(
            "Sessions",
            Some(session_count),
            l.overview.tab == Tab::Sessions,
            Tab::Sessions,
            "overview-tab-sessions",
            compact,
            k,
        ))
        .child(tab_pill(
            "Pull requests",
            (pr_count > 0).then_some(pr_count),
            l.overview.tab == Tab::PullRequests,
            Tab::PullRequests,
            "overview-tab-prs",
            compact,
            k,
        ))
        .child(tab_pill(
            "Worktrees",
            (worktree_count > 0).then_some(worktree_count),
            l.overview.tab == Tab::Worktrees,
            Tab::Worktrees,
            "overview-tab-worktrees",
            compact,
            k,
        ))
        .child(el().grow(1.0).min_w(0.0))
        .child(search)
        .children((l.overview.tab == Tab::Sessions).then(|| {
            quiet_button(
                &ui::ARCHIVE,
                if l.overview.archived {
                    "Show active sessions"
                } else {
                    "Show archived sessions"
                },
            )
            .expanded(l.overview.archived)
            .on_press(|s: &mut Workspace| {
                let o = &mut live_mut(s).overview;
                o.archived = !o.archived;
                o.cursor = None;
            })
            .build(k)
            .key("overview-archived")
        }))
        .child(
            quiet_button(&ui::REFRESH, "Refresh overview")
                .on_press(refresh_all)
                .build(k)
                .key("overview-refresh"),
        )
}
