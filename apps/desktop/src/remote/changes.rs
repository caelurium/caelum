use crate::{
    Workspace,
    live::{Live, live, live_mut},
    quiet_button, ui,
};
use caelum_core::remote::{Changes, Turn};
use rune::{El, TextEdit, col, row};
use rune_kit::{
    Kit,
    diff::{self, DiffState, DiffView},
    glyphs,
};

#[derive(Default)]
pub(super) struct Panel {
    key: Option<(String, String, String)>,
    owner: Option<String>,
    diff: DiffState,
    message: TextEdit,
}

pub(crate) fn open(s: &mut Workspace) {
    let l = live_mut(s);
    if let Some(id) = l.app.remote.saved.selected.clone() {
        l.app.review_remote(&id);
    }
    s.inspector.set_open(true);
}

pub(super) fn sync(l: &mut Live) -> bool {
    let id = &l.app.remote.saved.selected;
    let p = &mut l.remote.review;
    if p.owner.as_ref() != id.as_ref() {
        *p = Panel {
            owner: id.clone(),
            ..Default::default()
        };
        return true;
    }
    let Some(id) = id else { return false };
    if let Some(Changes::Ready(review)) = l.app.remote.changes.get(id) {
        let rev = &review.revision;
        if !p.key.as_ref().is_some_and(|(owner, head, tree)| {
            owner == id && head == &rev.head && tree == &rev.tree
        }) {
            p.diff = DiffState::new(diff::parse(&review.patch));
            p.key = Some((id.clone(), rev.head.clone(), rev.tree.clone()));
            return true;
        }
    }
    false
}

pub(crate) fn inspector(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let mut body = col().w_full().h_full().min_h(0.0).padding(10.0).gap(8.0);
    let Some((id, c)) = l
        .app
        .remote
        .saved
        .selected
        .as_ref()
        .and_then(|id| l.app.remote.saved.checkouts.get_key_value(id))
    else {
        return body;
    };
    body = body.child(
        row()
            .w_full()
            .center_y()
            .gap(4.0)
            .child(
                ui::caption(format!("Changes · Remote · {}", c.title()), k)
                    .grow(1.0)
                    .min_w(0.0)
                    .truncate(),
            )
            .child(ui::Button::new("Refresh").on_press(open).build(k))
            .child(
                quiet_button(&glyphs::CLOSE, "Hide remote changes")
                    .on_press(|s: &mut Workspace| s.inspector.set_open(false))
                    .build(k),
            ),
    );
    let review = match l.app.remote.changes.get(id) {
        Some(Changes::Ready(r)) => r,
        Some(Changes::Failed(e)) => {
            return body.child(ui::caption(e, k).wrap().color(k.c.danger.fg));
        }
        Some(Changes::Loading { .. }) => {
            return body.child(ui::caption("Reading the checkout on Boat…", k));
        }
        None => return body.child(ui::caption("Refresh to inspect this checkout.", k)),
    };
    let idle = matches!(c.phase, caelum_core::remote::Phase::Ready { .. })
        && c.thread.turn == Turn::Idle
        && !l.app.remote.jobs.contains_key(id);
    let a = id.clone();
    let b = id.clone();
    let status = review.status.lines().take(6).collect::<Vec<_>>().join("\n");
    body = body
        .child(ui::caption(&status, k).wrap().selectable(true))
        .child(
            ui::field(
                &l.remote.review.message,
                "Commit message",
                "Commit message",
                k,
            )
            .key(format!("remote-commit-message-{id}"))
            .on_edit(|s: &mut Workspace| &mut live_mut(s).remote.review.message),
        )
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(6.0)
                .child(
                    ui::Button::new("Commit reviewed")
                        .disabled(
                            !idle
                                || review.patch.is_empty()
                                || l.remote.review.message.text.trim().is_empty(),
                        )
                        .on_press(move |s: &mut Workspace| {
                            let l = live_mut(s);
                            l.app.commit_remote(&a, &l.remote.review.message.text);
                        })
                        .build(k)
                        .key("remote-commit")
                        .help("Commit exactly the changes reviewed on this machine"),
                )
                .child(
                    ui::Button::new("Push branch")
                        .disabled(!idle)
                        .on_press(move |s: &mut Workspace| live_mut(s).app.push_remote(&b))
                        .build(k)
                        .key("remote-push"),
                ),
        );
    if review.patch.is_empty() {
        body.child(ui::caption("No uncommitted changes", k))
    } else {
        body.child(
            DiffView::new(
                |s: &Workspace| &live(s).remote.review.diff,
                |s: &mut Workspace| &mut live_mut(s).remote.review.diff,
            )
            .key("remote-changes-diff")
            .build(s, k),
        )
    }
}
