use super::PULL_REQUEST;
use crate::{Workspace, checkouts, live::live, ui};
use rune::{Color, El, col, icon, row, text};
use rune_kit::Kit;

pub(super) fn view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let app = &l.app;
    let query = l.overview.search.text.trim().to_lowercase();
    let prs: Vec<_> = app
        .open_pull_requests()
        .into_iter()
        .filter(|(project, pr)| {
            if query.is_empty() {
                return true;
            }
            let name = checkouts::project_name(app, app.group_path(project));
            format!("{} #{} {} {}", pr.title, pr.number, name, pr.base_ref_name)
                .to_lowercase()
                .contains(&query)
        })
        .collect();
    if prs.is_empty() {
        return ui::empty(
            if query.is_empty() {
                "No open pull requests"
            } else {
                "No matching pull requests"
            },
            if query.is_empty() {
                "Open pull requests for active branches across your projects appear here."
            } else {
                "Try a different pull request title, number, project, or base branch."
            },
            k,
        );
    }
    let mut list = col()
        .key("overview-prs")
        .role("list")
        .label("Pull requests")
        .w_full()
        .gap(4.0)
        .focus_group();
    for (project, pr) in prs {
        let name = checkouts::project_name(app, app.group_path(project));
        let (status_label, status_tone) = if pr.is_draft {
            ("Draft", k.c.text_muted)
        } else {
            match pr.review_decision.as_deref() {
                Some("CHANGES_REQUESTED") => ("Changes requested", k.c.warning.fg),
                Some("APPROVED") => ("Approved", k.c.success.fg),
                Some("REVIEW_REQUIRED") => ("Review required", k.c.text_accent),
                _ => ("Open", k.c.text_accent),
            }
        };
        let project_owned = project.to_owned();
        let context_project = project_owned.clone();
        let url = pr.url.clone();
        let context_url = url.clone();
        let item = k
            .interactive(
                row()
                    .w_full()
                    .px(12.0)
                    .py(10.0)
                    .gap(10.0)
                    .center_y()
                    .radius(8.0)
                    .fill(Color::TRANSPARENT)
                    .child(icon(&PULL_REQUEST, 14.0).color(status_tone).a11y_hidden())
                    .child(
                        row()
                            .grow(1.0)
                            .min_w(0.0)
                            .center_y()
                            .gap(8.0)
                            .child(
                                text(&pr.title)
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate()
                                    .shrink()
                                    .min_w(0.0),
                            )
                            .child(
                                text(format!(
                                    "{name} · #{} · into {}",
                                    pr.number, pr.base_ref_name
                                ))
                                .font(k.ty.small)
                                .color(k.c.text_muted)
                                .truncate()
                                .shrink()
                                .min_w(0.0),
                            ),
                    )
                    .child(
                        text(status_label)
                            .font(k.ty.small_medium)
                            .color(status_tone)
                            .px(8.0)
                            .py(2.0)
                            .radius(10.0)
                            .fill(k.alpha(status_tone, 0.1))
                            .border(1.0, k.alpha(status_tone, 0.25)),
                    ),
            )
            .key(crate::code_view::source_key(&pr.url, "pull-request"))
            .role("button")
            .label(format!("#{} {}", pr.number, pr.title))
            .help(format!(
                "{name} · #{} · {}\ninto {}",
                pr.number, pr.title, pr.base_ref_name
            ))
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press(move |s: &mut Workspace| {
                crate::panels::open_review(s, project_owned.clone(), url.clone());
            });
        list = list.child(crate::context::pull_request(
            item,
            context_project,
            context_url,
        ));
    }
    list
}
