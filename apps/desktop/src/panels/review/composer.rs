use super::authoring::{Editor, Field, Owner, close};
use crate::live::{live, live_mut};
use crate::{
    Workspace, quiet_button,
    ui::{self, Button},
};
use caelum_protocol::{
    GitHubDiffSide, GitHubPendingReviewComment, GitHubReviewEvent, GitHubReviewWrite,
};
use rune::{El, ScrollSpec, col, row, text};
use rune_kit::{ButtonVariant, Kit, Placement, ScrollBar, diff, glyphs, popover};

const TRIGGER: &str = "review-composer-toggle";

pub(super) fn trigger(s: &Workspace, k: &Kit) -> Option<El> {
    let l = live(s);
    let owner = Owner::current(l)?;
    let count = l.app.services.reviews[&owner.key].draft.comments.len();
    Some(
        Button::new(if count == 0 {
            "Review".into()
        } else {
            format!("Review ({count})")
        })
        .variant(ButtonVariant::Outline)
        .disabled(owner.busy(l))
        .on_press(move |s: &mut Workspace| owner.open(s, Field::Summary))
        .build(k)
        .h(26.0)
        .px(9.0)
        .key(TRIGGER)
        .help("Write a pull request review"),
    )
}

pub(in crate::panels) fn comment_on_line(s: &mut Workspace, owner: &Owner, range: diff::LineRange) {
    let l = live(s);
    if !owner.matches(&l.panels, &l.app) {
        return;
    }
    let Some(file) = l.panels.diff.files().get(range.file) else {
        return;
    };
    let Some(row) = file
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .find(|row| row.number(range.end.side) == Some(range.end.line))
    else {
        return;
    };
    let (line, side) = match (row.new, row.old) {
        (Some(n), _) => (n, GitHubDiffSide::Right),
        (None, Some(n)) => (n, GitHubDiffSide::Left),
        _ => return,
    };
    owner.open(
        s,
        Field::Line {
            path: file.path().to_owned(),
            side,
            line,
        },
    );
}

fn location(path: &str, side: GitHubDiffSide, line: u32) -> String {
    let side = if side == GitHubDiffSide::Left {
        "old"
    } else {
        "new"
    };
    format!("{path}:{line} · {side} side")
}

pub(super) fn popover(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let Some(editor) = Editor::active(s) else {
        return;
    };
    let (title, body) = match &editor.field {
        Field::Reply(_) => return,
        Field::Summary => ("Submit review", summary(s, editor, k)),
        Field::Line { path, side, line } => (
            "Add review comment",
            col()
                .w_full()
                .gap(8.0)
                .child(ui::caption(location(path, *side, *line), k).wrap())
                .child(editor.input("review-line-input", "Comment on this line…", k))
                .child(
                    Button::new("Add to review")
                        .variant(ButtonVariant::Contrast)
                        .disabled(editor.text.text.trim().is_empty())
                        .on_press(queue_line)
                        .build(k)
                        .key("review-queue-line"),
                ),
        ),
    };
    let width =
        rune::memo::window_layout("shell").map_or(380.0, |r| (r[2] - 24.0).clamp(1.0, 380.0));
    let popup = col()
        .key("review-composer")
        .role("dialog")
        .label(title)
        .focus_group()
        .modal(true, close)
        .focus_return(TRIGGER)
        .w(width)
        .min_h(0.0)
        .padding(12.0)
        .gap(8.0)
        .child(
            row()
                .w_full()
                .center_y()
                .justify_between()
                .child(text(title).font(k.ty.ui_medium).color(k.c.text_base))
                .child(
                    quiet_button(&glyphs::CLOSE, "Close review editor")
                        .on_press(close)
                        .build(k)
                        .key("review-composer-close"),
                ),
        )
        .child(body);
    layers.push(ui::popup_scrim("review-composer-scrim", close));
    layers.push(popover::anchored(
        k,
        popover::raised(k, popup, ui::POPUP_RADIUS),
        TRIGGER,
        Placement::BottomEnd,
        ui::POPUP_OFFSET,
    ));
}

fn queue_line(s: &mut Workspace) {
    let Some(e) = Editor::active(s) else { return };
    let Field::Line { path, side, line } = &e.field else {
        return;
    };
    if e.text.text.trim().is_empty() {
        return;
    }
    let comment = GitHubPendingReviewComment {
        path: path.clone(),
        side: *side,
        line: *line,
        body: e.text.text.clone(),
    };
    let owner = e.owner.clone();
    if let Some(r) = owner.editable(live_mut(s)) {
        r.draft.comments.push(comment);
        close(s);
    }
}

fn summary(s: &Workspace, editor: &Editor, k: &Kit) -> El {
    let l = live(s);
    let owner = &editor.owner;
    let draft = &l.app.services.reviews[&owner.key].draft;
    let mut choices = ui::segmented_track(k)
        .self_start()
        .role("radiogroup")
        .label("Review decision");
    for (event, name) in [
        (GitHubReviewEvent::Comment, "Comment"),
        (GitHubReviewEvent::Approve, "Approve"),
        (GitHubReviewEvent::RequestChanges, "Request changes"),
    ] {
        let owner = owner.clone();
        choices = choices.child(
            ui::segment(name, event == draft.event, k)
                .role("radio")
                .on_press(move |s: &mut Workspace| {
                    if let Some(r) = owner.editable(live_mut(s)) {
                        r.draft.event = event
                    }
                })
                .key(format!("review-event-{event:?}")),
        );
    }
    let mut content = col().w_full().gap(8.0).child(choices).child(editor.input(
        "review-summary-input",
        if draft.event == GitHubReviewEvent::RequestChanges {
            "Explain the requested changes…"
        } else {
            "Review summary…"
        },
        k,
    ));
    if !draft.comments.is_empty() {
        let count = draft.comments.len();
        let lookup = owner.clone();
        let kit = k.clone();
        let list = col()
            .w_full()
            .growth(rune::GrowthSpec::default())
            .virtual_rows(
                count,
                44.0,
                |_: &Workspace, i| format!("pending-review-{i}").into(),
                move |s: &Workspace, i| {
                    let item = &live(s).app.services.reviews[&lookup.key].draft.comments[i];
                    let remove = lookup.clone();
                    row()
                        .w_full()
                        .h(44.0)
                        .center_y()
                        .gap(6.0)
                        .child(
                            col()
                                .grow(1.0)
                                .min_w(0.0)
                                .child(
                                    ui::caption(location(&item.path, item.side, item.line), &kit)
                                        .truncate(),
                                )
                                .child(
                                    text(&item.body)
                                        .font(kit.ty.ui)
                                        .color(kit.c.text_base)
                                        .truncate()
                                        .help(&item.body),
                                ),
                        )
                        .child(
                            quiet_button(&glyphs::CLOSE, "Remove pending comment")
                                .on_press(move |s: &mut Workspace| {
                                    if let Some(r) = remove.editable(live_mut(s))
                                        && i < r.draft.comments.len()
                                    {
                                        r.draft.comments.remove(i);
                                    }
                                })
                                .build(&kit),
                        )
                },
            );
        content = content
            .child(ui::caption(format!("Pending comments ({count})"), k))
            .child(
                col()
                    .w_full()
                    .h((count.min(3) * 44) as f32)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(list)
                    .scroll_bar(k),
            );
    }
    let owner = owner.clone();
    content.child(
        Button::new("Submit review")
            .variant(ButtonVariant::Contrast)
            .disabled(!draft.can_submit() || owner.busy(l))
            .on_press(move |s: &mut Workspace| {
                if let Some(r) = owner.editable(live_mut(s)) {
                    let action = GitHubReviewWrite::Submit(r.draft.clone());
                    owner.send(s, action);
                }
            })
            .build(k)
            .key("review-submit"),
    )
}
