use super::live;
use crate::{Workspace, panels, ui};
use rune::{El, icon, row, text};
use rune_kit::Kit;

/// Show one next action beside the conversation title. A direct request for the
/// user's input takes precedence over review feedback.
pub(crate) fn session_attention(s: &Workspace, k: &Kit) -> Option<El> {
    let l = s.live.as_ref()?;
    if l.panels.screen != panels::Screen::Conversation {
        return None;
    }
    let index = l.app.selected?;
    let session = &l.app.sessions[index];
    if l.app.needs_input(session) {
        return Some(
            k.interactive(
                row()
                    .center_y()
                    .gap(5.0)
                    .px(8.0)
                    .h(24.0)
                    .radius(6.0)
                    .fill(k.alpha(k.c.warning.fg, 0.12))
                    .border(1.0, k.alpha(k.c.warning.fg, 0.28))
                    .child(
                        rune::el()
                            .size(6.0, 6.0)
                            .radius(3.0)
                            .fill(k.c.warning.fg)
                            .a11y_hidden(),
                    )
                    .child(
                        ui::caption("Needs input", k)
                            .font(k.ty.small_medium)
                            .color(k.c.warning.fg)
                            .role("status")
                            .label("Needs input")
                            .key("session-title-input"),
                    ),
            )
            .key("session-title-input-button")
            .role("button")
            .label("Needs input")
            .help("Jump to pending approval or question")
            .focusable(true)
            .focus_ring(1.5, 0.0, k.c.border_focus)
            .on_press_tx(move |_: &mut Workspace, tx: &mut rune::Tx| {
                tx.scroll_to_bottom_instant(format!("live-transcript-{index}"));
            }),
        );
    }
    let pr = l.app.requested_changes(session)?;
    let label = format!("PR #{} · Changes requested", pr.number);
    let project = session.scope.project.clone();
    let url = pr.url.clone();
    Some(
        k.interactive(
            row()
                .center_y()
                .gap(5.0)
                .px(6.0)
                .h(26.0)
                .radius(6.0)
                .child(icon(&ui::BRANCH, 13.0).color(k.c.text_accent))
                .child(text(&label).font(k.ty.small_medium).color(k.c.text_accent)),
        )
        .key("session-title-review")
        .role("button")
        .label(label)
        .help(format!("Open pull request: {}", pr.title))
        .focusable(true)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .on_press(move |s: &mut Workspace| {
            if live(s).app.selected == Some(index)
                && live(s).panels.screen == panels::Screen::Conversation
            {
                panels::open_review(s, project.clone(), url.clone());
            }
        }),
    )
}
