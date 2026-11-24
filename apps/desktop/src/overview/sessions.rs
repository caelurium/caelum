use std::rc::Rc;

use caelum_core::{
    application::{
        FeedSession, OverviewActivity, SessionBucket,
        presentation::{TurnPreview, excerpt},
    },
    remote::Turn,
};
use caelum_protocol::GitHubPullRequest;
use rune::{Color, El, Icon, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, glyphs};

use super::{PULL_REQUEST, RUNNING, SETTLED, WAITING, choose, row_key};
use crate::{
    Workspace, checkouts,
    live::{Live, live, live_mut},
    ui,
};

const ROW_HEIGHT: f32 = 46.0;

#[derive(Clone, PartialEq, Eq, Hash)]
enum ListRow {
    Header {
        bucket: SessionBucket,
        count: usize,
    },
    Session {
        id: FeedSession,
        bucket: SessionBucket,
    },
}

pub(super) fn ordered_sessions(l: &Live) -> Vec<FeedSession> {
    let all = l
        .app
        .dashboard_sessions(&l.overview.search.text, l.overview.archived);
    let mut waiting = Vec::new();
    let mut running = Vec::new();
    let mut settled = Vec::new();
    for id in all {
        match l.app.session_bucket(&id) {
            SessionBucket::Waiting => waiting.push(id),
            SessionBucket::Running => running.push(id),
            SessionBucket::Settled => settled.push(id),
        }
    }
    let mut ordered = Vec::with_capacity(waiting.len() + running.len() + settled.len());
    if !l.overview.is_collapsed(SessionBucket::Waiting) {
        ordered.extend(waiting);
    }
    if !l.overview.is_collapsed(SessionBucket::Running) {
        ordered.extend(running);
    }
    if !l.overview.is_collapsed(SessionBucket::Settled) {
        ordered.extend(settled);
    }
    ordered
}

pub(super) fn view(s: &Workspace, compact: bool, k: &Kit) -> El {
    let l = live(s);
    let all = l
        .app
        .dashboard_sessions(&l.overview.search.text, l.overview.archived);
    if all.is_empty() {
        let (title, detail) = if !l.overview.search.text.trim().is_empty() {
            (
                "No matching sessions",
                "Try a different title, project, provider, or branch.",
            )
        } else if l.overview.archived {
            (
                "No archived sessions",
                "Archived conversations across open projects appear here.",
            )
        } else if l.app.projects.is_empty() {
            (
                "No open projects",
                "Open a project to see sessions, pull requests, and worktrees here.",
            )
        } else {
            (
                "No sessions yet",
                "Start a conversation from Home or press ⌘N.",
            )
        };
        return ui::empty(title, detail, k);
    }
    let (mut waiting, mut running, mut settled) = (Vec::new(), Vec::new(), Vec::new());
    for id in all {
        match l.app.session_bucket(&id) {
            SessionBucket::Waiting => waiting.push(id),
            SessionBucket::Running => running.push(id),
            SessionBucket::Settled => settled.push(id),
        }
    }
    let mut flat = Vec::new();
    for (bucket, group) in [
        (SessionBucket::Waiting, waiting),
        (SessionBucket::Running, running),
        (SessionBucket::Settled, settled),
    ] {
        if group.is_empty() {
            continue;
        }
        flat.push(ListRow::Header {
            bucket,
            count: group.len(),
        });
        if !l.overview.is_collapsed(bucket) {
            for id in group {
                flat.push(ListRow::Session { id, bucket });
            }
        }
    }
    let selected = l
        .overview
        .cursor
        .as_ref()
        .filter(|id| {
            flat.iter()
                .any(|r| matches!(r, ListRow::Session { id: row, .. } if row == *id))
        })
        .cloned();
    let flat = Rc::new(flat);
    let keys = flat.clone();
    let rows = flat.clone();
    let kit = k.clone();
    col()
        .w_full()
        .key("overview-rows")
        .role("list")
        .label("Sessions")
        .growth(rune::GrowthSpec::default())
        .virtual_rows(
            flat.len(),
            ROW_HEIGHT,
            move |_: &Workspace, i| match &keys[i] {
                ListRow::Header { bucket, .. } => format!("overview-group-{bucket:?}").into(),
                ListRow::Session { id, .. } => row_key(id).into(),
            },
            move |s: &Workspace, i| match &rows[i] {
                ListRow::Header { bucket, count } => group_header(s, *bucket, *count, &kit),
                ListRow::Session { id, bucket } => {
                    session_row(s, id, *bucket, selected.as_ref() == Some(id), compact, &kit)
                }
            },
        )
        .virtual_rows_version(flat.as_ref())
}

fn group_meta(bucket: SessionBucket, k: &Kit) -> (&'static str, &'static Icon, Color) {
    match bucket {
        SessionBucket::Waiting => ("Waiting for you", &WAITING, k.c.warning.fg),
        SessionBucket::Running => ("Running", &RUNNING, k.c.text_accent),
        SessionBucket::Settled => ("Settled", &SETTLED, k.c.success.fg),
    }
}

fn group_header(s: &Workspace, bucket: SessionBucket, count: usize, k: &Kit) -> El {
    let collapsed = live(s).overview.is_collapsed(bucket);
    let (title, glyph, tone) = group_meta(bucket, k);
    let chevron = if collapsed {
        &glyphs::CARET_RIGHT
    } else {
        &glyphs::CHEVRON_DOWN
    };
    col().w_full().pt(8.0).pb(4.0).child(
        k.interactive(
            row()
                .w_full()
                .h(34.0)
                .px(10.0)
                .gap(8.0)
                .center_y()
                .radius(8.0)
                .fill(k.c.layer_01)
                .border(1.0, k.alpha(k.c.border_base, 0.7))
                .child(icon(chevron, 11.0).color(k.c.icon_muted).a11y_hidden())
                .child(icon(glyph, 13.0).color(tone).a11y_hidden())
                .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
                .child(
                    text(count.to_string())
                        .font(k.ty.small)
                        .color(k.c.text_muted),
                ),
        )
        .role("button")
        .label(format!("{title} {count}"))
        .expanded(!collapsed)
        .focusable(true)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press(move |s: &mut Workspace| live_mut(s).overview.toggle_bucket(bucket)),
    )
}

fn local_excerpt(s: &Workspace, index: usize, k: &Kit) -> Option<(String, Color)> {
    let l = live(s);
    let session = &l.app.sessions[index];
    if let Some(activity) = l.app.overview_activity(session) {
        return match activity {
            OverviewActivity::Question => {
                let text = crate::questions::preview(l, index)
                    .unwrap_or_else(|| "Question waiting for your answer".into());
                Some((format!("Needs input · {text}"), k.c.warning.fg))
            }
            OverviewActivity::Approval(summary) => Some((
                format!("Approval needed · {}", excerpt(summary)),
                k.c.warning.fg,
            )),
            OverviewActivity::Agent => {
                l.overview.watch_turn(index);
                let preview = l.app.turn_preview(session);
                Some((format_turn_preview(l, &preview), k.c.text_accent))
            }
            OverviewActivity::Command => {
                Some(("Running a provider command".into(), k.c.text_accent))
            }
            OverviewActivity::SideQuestion => {
                let text = session
                    .side_question
                    .as_ref()
                    .map(|(q, _)| format!("Side question · {}", excerpt(q)))
                    .unwrap_or_else(|| "Answering a side question".into());
                Some((text, k.c.text_accent))
            }
            OverviewActivity::Queued(prompt) => Some((
                format!(
                    "Queued · {}",
                    excerpt(crate::panels::queued_preview(prompt))
                ),
                k.c.text_muted,
            )),
        };
    }
    if let Some(pr) = l.app.requested_changes(session) {
        return Some((
            format!("PR #{} · Changes requested", pr.number),
            k.c.warning.fg,
        ));
    }
    if session.queue_paused && !session.queue.is_empty() {
        return Some((
            format!("{} queued (paused)", session.queue.len()),
            k.c.warning.fg,
        ));
    }
    if let Some(err) = session.error.as_deref().or_else(|| {
        session
            .conversation
            .failure
            .notice()
            .map(|n| n.message.as_str())
    }) {
        return Some((excerpt(err), k.c.danger.fg));
    }
    None
}

fn format_turn_preview(l: &Live, preview: &TurnPreview) -> String {
    let age = preview
        .started_at
        .and_then(|at| l.overview.frame.borrow_mut().age(at, l.feed.displayed_at()));
    let head = age.map_or_else(
        || preview.label.to_owned(),
        |age| format!("{} · {age}", preview.label),
    );
    match preview.text.as_deref().filter(|t| !t.is_empty()) {
        Some(snippet) => format!("{head} · {snippet}"),
        None => head,
    }
}

fn format_age(at: Option<u64>, now: u64) -> String {
    let Some(at) = at else {
        return String::new();
    };
    let elapsed = now.saturating_sub(at);
    if elapsed < 60_000 {
        "now".into()
    } else if elapsed < 3_600_000 {
        format!("{}m", elapsed / 60_000)
    } else if elapsed < 86_400_000 {
        format!("{}h", elapsed / 3_600_000)
    } else {
        format!("{}d", elapsed / 86_400_000)
    }
}

fn pr_pill(project: String, pr: &GitHubPullRequest, k: &Kit) -> El {
    let tone = if pr.is_draft {
        k.c.text_muted
    } else if pr.review_decision.as_deref() == Some("CHANGES_REQUESTED") {
        k.c.warning.fg
    } else if pr.review_decision.as_deref() == Some("APPROVED") {
        k.c.success.fg
    } else {
        k.c.text_accent
    };
    let url = pr.url.clone();
    let label = format!("#{}", pr.number);
    k.interactive(
        row()
            .center_y()
            .gap(4.0)
            .h(22.0)
            .px(7.0)
            .radius(11.0)
            .fill(k.alpha(tone, 0.1))
            .border(1.0, k.alpha(tone, 0.28))
            .child(icon(&PULL_REQUEST, 11.0).color(tone).a11y_hidden())
            .child(text(&label).font(k.ty.small_medium).color(tone)),
    )
    .role("button")
    .label(format!("Open pull request #{}: {}", pr.number, pr.title))
    .help(format!("#{} · {}", pr.number, pr.title))
    .focusable(true)
    .on_press(move |s: &mut Workspace| {
        crate::panels::open_review(s, project.clone(), url.clone());
    })
}

fn session_row(
    s: &Workspace,
    id: &FeedSession,
    bucket: SessionBucket,
    highlighted: bool,
    compact: bool,
    k: &Kit,
) -> El {
    let l = live(s);
    let app = &l.app;
    let (_, glyph, tone) = group_meta(bucket, k);
    let (title, meta, provider, age, excerpt_line, pr_badge, available, local_index) = match id {
        FeedSession::Local(index) => {
            let session = &app.sessions[*index];
            let project = checkouts::project_name(app, app.group_path(&session.scope.project));
            let branch = app.session_branch(session);
            let worktree = app.session_worktree(session);
            let meta = match (worktree, branch) {
                (Some(tree), Some(b)) => format!("{project} · {tree} · {b}"),
                (Some(tree), None) => format!("{project} · {tree}"),
                (None, Some(b)) => format!("{project} · {b}"),
                (None, None) => project.to_owned(),
            };
            let pr = app
                .session_pull_request(session)
                .map(|pr| (session.scope.project.clone(), pr));
            let detail = if bucket != SessionBucket::Settled {
                local_excerpt(s, *index, k)
            } else {
                None
            };
            (
                session.title.clone(),
                meta,
                ui::provider_name(session.scope.provider),
                format_age(session.last_activity, l.feed.displayed_at()),
                detail,
                pr,
                !session.archiving() && !session.deleting(),
                Some(*index),
            )
        }
        FeedSession::Vm(link) => {
            let Some((checkout, thread)) = link.resolve(&app.remote.saved) else {
                return el();
            };
            let project = checkouts::project_name(app, app.group_path(&checkout.source.project));
            let meta = match checkout.branch() {
                Some(b) => format!("{project} · VM {} · {b}", checkout.title()),
                None => format!("{project} · VM {}", checkout.title()),
            };
            let detail = if thread.turn != Turn::Idle {
                TurnPreview::managed(thread).map(|p| {
                    let tone = if matches!(thread.turn, Turn::Queued(_)) {
                        k.c.text_muted
                    } else {
                        k.c.text_accent
                    };
                    (format_turn_preview(l, &p), tone)
                })
            } else {
                None
            };
            (
                thread.title().to_owned(),
                meta,
                thread.provider.name(),
                format_age(thread.last_activity, l.feed.displayed_at()),
                detail,
                None,
                app.can_open_vm_thread(link),
                None,
            )
        }
    };

    let primary = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(icon(glyph, 13.0).color(tone).a11y_hidden())
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .center_y()
                .gap(8.0)
                .child(
                    text(&title)
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .truncate()
                        .shrink()
                        .min_w(0.0),
                )
                .child(
                    text(&meta)
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .truncate()
                        .shrink()
                        .min_w(0.0),
                ),
        )
        .children(pr_badge.map(|(project, pr)| pr_pill(project, pr, k)))
        .children((!compact).then(|| {
            text(provider)
                .font(TextStyle::new(10.5, 450.0))
                .color(k.c.text_muted)
                .px(7.0)
                .py(2.0)
                .radius(10.0)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_base)
        }))
        .children((!age.is_empty()).then(|| {
            text(&age)
                .font(k.ty.small)
                .color(k.c.text_faint)
                .min_w(26.0)
        }));

    let mut content = col()
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(3.0)
        .radius(8.0)
        .fill(if highlighted {
            k.c.hover
        } else {
            Color::TRANSPARENT
        })
        .border(
            1.0,
            if highlighted {
                k.c.border_focus
            } else {
                Color::TRANSPARENT
            },
        )
        .child(primary);
    if let Some((snippet, snippet_tone)) = excerpt_line {
        content = content.child(
            text(snippet)
                .font(k.ty.small)
                .color(snippet_tone)
                .pl(21.0)
                .truncate()
                .w_full(),
        );
    }

    let owner = id.clone();
    let item = k
        .interactive(content)
        .key(row_key(id))
        .role("button")
        .label(&title)
        .description(format!("{title}, {meta}, {provider}, {age}"))
        .disabled(!available)
        .focusable(available)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press_tx(move |s: &mut Workspace, tx| choose(s, &owner, tx));
    let item = col().w_full().pb(2.0).child(item);
    match local_index {
        Some(index) => crate::context::session(item, index),
        None => item,
    }
}
