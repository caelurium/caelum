use super::{choose, tile_key};
use crate::{Workspace, checkouts, live::live, ui};
use caelum_core::{
    application::{
        EntryKind, FeedSession, OverviewActivity,
        presentation::{ActivityKind, excerpt},
    },
    remote::Turn,
};
use rune::{Color, El, Icon, TextStyle, col, el, icon, row, text};
use rune_kit::Kit;

enum Note {
    Question(String),
    Approval(String),
    Working(String),
    Queued(String),
}

impl Note {
    fn style(&self, k: &Kit) -> (&'static str, &'static Icon, Color, &str) {
        match self {
            Self::Question(v) => ("Needs your input", &ui::BELL, k.c.warning.fg, v),
            Self::Approval(v) => ("Approval needed", &ui::BELL, k.c.warning.fg, v),
            Self::Working(v) => ("In progress", &ui::CLOCK, k.c.text_accent, v),
            Self::Queued(v) => ("Queued", &ui::CLOCK, k.c.text_muted, v),
        }
    }
}

fn local_note(s: &Workspace, index: usize) -> Option<Note> {
    let l = live(s);
    let session = &l.app.sessions[index];
    Some(match l.app.overview_activity(session)? {
        OverviewActivity::Question => Note::Question(crate::questions::preview(l, index)?),
        OverviewActivity::Approval(summary) => Note::Approval(excerpt(summary)),
        OverviewActivity::Agent => {
            let activity = crate::activity::session_activity(s, index);
            let task = activity
                .as_ref()
                .filter(|a| a.kind == ActivityKind::Background)
                .and_then(|_| {
                    session
                        .conversation
                        .entries
                        .iter()
                        .rev()
                        .take_while(|e| e.kind() != EntryKind::User)
                        .find(|e| e.name == "subagent" && !e.complete())
                })
                .and_then(caelum_core::application::subagents::Subagent::from_entry);
            Note::Working(match task {
                Some(t) => excerpt(&t.description),
                None => activity.map_or_else(|| "Working…".into(), |a| a.label),
            })
        }
        OverviewActivity::Command => Note::Working("Running a provider command…".into()),
        OverviewActivity::SideQuestion => Note::Working("Answering a side question…".into()),
        OverviewActivity::Queued(prompt) => {
            Note::Queued(excerpt(crate::panels::queue::queued_preview(prompt)))
        }
    })
}

fn vm_note(t: &caelum_core::remote::Thread) -> Option<Note> {
    let status = match t.turn {
        Turn::Idle => return None,
        Turn::Queued(_) => return Some(Note::Queued("Waiting to run on the VM…".into())),
        Turn::Submitting => "Sending the prompt…",
        Turn::Following(_) => "Working on the VM…",
        Turn::Finishing(_) => "Receiving final updates…",
        Turn::Stopping(_) => "Stopping the turn…",
    };
    Some(Note::Working(status.into()))
}

pub(super) fn tile(
    s: &Workspace,
    id: &FeedSession,
    highlighted: bool,
    compact: bool,
    k: &Kit,
) -> El {
    let l = live(s);
    let app = &l.app;
    let (title, project, destination, provider, note, available) = match id {
        FeedSession::Local(i) => {
            let session = &app.sessions[*i];
            (
                session.title.as_str(),
                session.scope.project.as_str(),
                app.session_worktree(session),
                session.scope.provider,
                local_note(s, *i),
                !session.archiving() && !session.deleting(),
            )
        }
        FeedSession::Vm(link) => {
            let Some((checkout, t)) = link.resolve(&app.remote.saved) else {
                return el();
            };
            (
                t.title(),
                checkout.source.project.as_str(),
                Some(checkout.title()),
                t.provider,
                vm_note(t),
                app.can_open_vm_thread(link),
            )
        }
    };
    let Some(note) = note else {
        return el();
    };
    let name = checkouts::project_name(app, app.group_path(project));
    let identity = destination.map_or_else(|| name.to_owned(), |d| format!("{name} / {d}"));
    let ink = l.feed.project_ink(app, project, k);
    let (label, glyph, tone, snippet) = note.style(k);
    let owner = id.clone();
    k.interactive(
        col()
            .w_full()
            .h_full()
            .px(if compact { 14.0 } else { 18.0 })
            .py(if compact { 11.0 } else { 16.0 })
            .gap(if compact { 4.0 } else { 7.0 })
            .radius(12.0)
            .fill(k.c.layer_01)
            .border(
                if highlighted { 1.5 } else { 1.0 },
                if highlighted {
                    k.c.border_focus
                } else {
                    k.c.border_muted
                },
            )
            .child(
                row()
                    .w_full()
                    .center_y()
                    .gap(6.0)
                    .child(el().size(7.0, 7.0).radius(3.5).fill(ink))
                    .child(
                        text(&identity)
                            .font(k.ty.small_medium)
                            .color(ink)
                            .truncate()
                            .min_w(0.0)
                            .grow(1.0),
                    )
                    .child(ui::caption(ui::provider_name(provider), k)),
            )
            .child(
                text(title)
                    .font(TextStyle::new(if compact { 16.0 } else { 18.0 }, 580.0).lh(23.0))
                    .color(k.c.text_base)
                    .w_full()
                    .truncate(),
            )
            .child(
                text(snippet)
                    .font(TextStyle::new(13.0, 400.0).lh(19.0))
                    .color(k.c.text_base)
                    .w_full()
                    .wrap_anywhere()
                    .max_lines(2),
            )
            .child(el().grow(1.0))
            .child(
                row()
                    .center_y()
                    .gap(6.0)
                    .child(icon(glyph, 12.0).color(tone).a11y_hidden())
                    .child(text(label).font(k.ty.small_medium).color(tone)),
            ),
    )
    .key(tile_key(id))
    .role("button")
    .label(title)
    .description(format!(
        "{identity}, {}, {label}: {snippet}",
        provider.name()
    ))
    .disabled(!available)
    .focusable(available)
    .focus_ring(2.0, 1.0, k.c.border_focus)
    .on_key(super::key)
    .on_press_tx(move |s: &mut Workspace, tx| choose(s, &owner, tx))
}
