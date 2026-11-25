use super::authoring::{Editor, Field, Owner, close};
use crate::Workspace;
use crate::live::live;
use crate::ui::Button;
use caelum_protocol::{GitHubReviewThread, GitHubReviewWrite};
use rune::{El, col, row};
use rune_kit::{ButtonVariant, Kit};

pub(in crate::panels) fn thread_actions(
    s: &Workspace,
    k: &Kit,
    thread: &GitHubReviewThread,
) -> Option<El> {
    let owner = Owner::current(live(s))?;
    let busy = owner.busy(live(s));
    let field = Field::Reply(thread.id.clone());
    let editor = Editor::active(s).filter(|e| e.field == field);
    let mut actions = row().w_full().flex_wrap().gap(6.0);
    if thread.viewer_can_reply {
        let replying = editor.is_some();
        let owner = owner.clone();
        actions = actions.child(
            Button::new(if replying { "Close reply" } else { "Reply" })
                .variant(ButtonVariant::GhostMuted)
                .disabled(busy)
                .on_press_tx(move |s: &mut Workspace, tx| {
                    if replying {
                        close(s);
                    } else {
                        owner.open(s, field.clone());
                        tx.scroll_into_view("thread-reply-form");
                    }
                })
                .build(k)
                .h(26.0)
                .px(8.0)
                .key(format!("thread-reply-{}", thread.id)),
        );
    }
    let resolved = !thread.is_resolved;
    if thread.can_toggle_resolution() {
        let id = thread.id.clone();
        let owner = owner.clone();
        actions = actions.child(
            Button::new(if resolved { "Resolve" } else { "Unresolve" })
                .variant(ButtonVariant::GhostMuted)
                .disabled(busy)
                .on_press(move |s: &mut Workspace| {
                    owner.send(
                        s,
                        GitHubReviewWrite::SetResolved {
                            thread_id: id.clone(),
                            resolved,
                        },
                    )
                })
                .build(k)
                .h(26.0)
                .px(8.0)
                .key(format!("thread-resolve-{}", thread.id)),
        );
    } else if !thread.viewer_can_reply {
        return None;
    }
    let Some(editor) = editor else {
        return Some(actions);
    };
    let id = thread.id.clone();
    Some(
        col()
            .key("thread-reply-form")
            .w_full()
            .gap(6.0)
            .child(actions)
            .child(editor.input("thread-reply-input", "Write a reply…", k))
            .child(
                Button::new("Post reply")
                    .variant(ButtonVariant::Contrast)
                    .disabled(busy || editor.text.text.trim().is_empty())
                    .on_press(move |s: &mut Workspace| {
                        let Some(e) = Editor::active(s)
                            .filter(|e| matches!(&e.field, Field::Reply(thread) if thread == &id))
                        else {
                            return;
                        };
                        let body = e.text.text.clone();
                        owner.send(
                            s,
                            GitHubReviewWrite::Reply {
                                thread_id: id.clone(),
                                body,
                            },
                        );
                    })
                    .build(k)
                    .key("thread-reply-submit"),
            ),
    )
}
