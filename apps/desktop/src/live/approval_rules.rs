use super::{Live, live, live_mut};
use crate::{
    Workspace,
    panels::Screen,
    ui::{self, Button},
};
use caelum_protocol::{ApprovalDecision, ApprovalPrompt};
use rune::{El, ScrollSpec, col, row};
use rune_kit::{ButtonVariant, Kit, Placement, ScrollBar};

const TRIGGER: &str = "session-approval-rules";

pub(super) fn trigger(l: &Live, index: usize, k: &Kit) -> Option<El> {
    let owner = l.app.sessions[index].key()?;
    let n = l.app.approval_rules(&owner).len();
    (n > 0).then(|| {
        Button::new(format!(
            "{n} auto-allow rule{}",
            if n == 1 { "" } else { "s" }
        ))
        .variant(ButtonVariant::Outline)
        .on_press(move |s: &mut Workspace| {
            live_mut(s).approval_menu = Some(owner.clone());
        })
        .build(k)
        .h(24.0)
        .px(7.0)
        .key(TRIGGER)
    })
}

pub(super) fn sync(l: &mut Live) {
    if l.approval_menu.as_ref().is_some_and(|owner| {
        l.panels.screen != Screen::Conversation
            || l.panels.overlay().is_some()
            || l.app
                .selected
                .and_then(|i| l.app.sessions[i].key())
                .as_ref()
                != Some(owner)
            || l.app.approval_rules(owner).is_empty()
    }) {
        l.approval_menu = None;
    }
}

pub(super) fn overlay(s: &Workspace, k: &Kit, layers: &mut Vec<El>) {
    let l = live(s);
    let Some(owner) = l.approval_menu.as_ref() else {
        return;
    };
    let mut list = col().w_full().gap(4.0);
    for (i, rule) in l.app.approval_rules(owner).iter().enumerate() {
        let key = owner.clone();
        let remove = rule.clone();
        list = list.child(
            row()
                .w_full()
                .center_y()
                .gap(6.0)
                .child(ui::caption(rule.to_string(), k).wrap().grow(1.0).min_w(0.0))
                .child(
                    Button::new("Remove")
                        .variant(ButtonVariant::GhostMuted)
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s).app.revoke_approval_rules(&key, Some(&remove))
                        })
                        .build(k)
                        .key(format!("revoke-rule-{i}")),
                ),
        );
    }
    let key = owner.clone();
    let body = col()
        .w(316.0)
        .gap(6.0)
        .child(ui::popup_header(
            "Session approval rules",
            Some("Applies only to this conversation until Caelum restarts."),
            None,
            k,
        ))
        .child(
            col()
                .w_full()
                .max_h(200.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(list)
                .scroll_bar(k),
        )
        .child(
            Button::new("Clear rules")
                .variant(ButtonVariant::Outline)
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    l.app.revoke_approval_rules(&key, None);
                    l.approval_menu = None;
                })
                .build(k)
                .key("clear-approval-rules"),
        );
    ui::popup(
        "approval-rules-popover",
        TRIGGER,
        body,
        Placement::BottomEnd,
        |s: &mut Workspace| live_mut(s).approval_menu = None,
        k,
        layers,
    );
}

pub(super) fn actions(l: &Live, prompt: &ApprovalPrompt, k: &Kit) -> El {
    let busy = l.app.scoped_approval_pending(prompt) || !crate::remote::native::ready(l);
    let allow = prompt.clone();
    let grant = prompt.clone();
    let deny = prompt.clone();
    let detail = prompt.repeatable.as_ref().map(ToString::to_string);
    col()
        .w_full()
        .gap(6.0)
        .children(
            detail
                .as_ref()
                .map(|d| ui::caption(format!("Session rule: {d}"), k).wrap()),
        )
        .child(
            row()
                .w_full()
                .flex_wrap()
                .gap(8.0)
                .center_y()
                .child(
                    Button::new("Allow once")
                        .variant(ButtonVariant::Contrast)
                        .disabled(busy)
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s)
                                .app
                                .approve(allow.clone(), ApprovalDecision::AllowOnce)
                        })
                        .build(k)
                        .key(format!("allow-once-{}", prompt.request_id)),
                )
                .child(
                    Button::new("Allow similar in session")
                        .variant(ButtonVariant::Outline)
                        .disabled(busy || !l.app.can_allow_similar(prompt))
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s).app.allow_similar(grant.clone())
                        })
                        .build(k)
                        .help(detail.unwrap_or_else(|| {
                            "This request cannot be matched by a session rule.".into()
                        }))
                        .key(format!("allow-similar-{}", prompt.request_id)),
                )
                .child(
                    Button::new("Deny")
                        .variant(ButtonVariant::Outline)
                        .disabled(busy)
                        .on_press(move |s: &mut Workspace| {
                            live_mut(s)
                                .app
                                .approve(deny.clone(), ApprovalDecision::Deny)
                        })
                        .build(k)
                        .key(format!("deny-{}", prompt.request_id)),
                ),
        )
}
