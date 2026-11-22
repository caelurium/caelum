use super::{choose, tile_key};
use crate::{Workspace, checkouts, live::live, ui};
use caelum_core::{
    application::{
        EntryKind, FeedSession,
        presentation::{excerpt, latest_message, visible_text},
    },
    remote::{Message, Speaker, Turn},
};
use rune::{Color, El, Icon, TextStyle, col, el, icon, row, text};
use rune_kit::{Kit, glyphs};

enum Note {
    Question(String),
    Approval(String),
    Working(String),
    Draft(String),
    Reply(String),
    Prompt(String),
    Error(String),
    Empty(&'static str),
}

impl Note {
    fn style(&self, k: &Kit) -> (&'static str, &'static Icon, Color, &str) {
        match self {
            Self::Question(v) => ("Needs your input", &ui::BELL, k.c.warning.fg, v),
            Self::Approval(v) => ("Approval needed", &ui::BELL, k.c.warning.fg, v),
            Self::Working(v) => ("In progress", &ui::CLOCK, k.c.text_accent, v),
            Self::Draft(v) => ("Unsent draft", &ui::EDIT, k.c.text_muted, v),
            Self::Reply(v) => ("Latest reply", &glyphs::SPARKLE, k.c.text_muted, v),
            Self::Prompt(v) => ("Last prompt", &ui::EDIT, k.c.text_muted, v),
            Self::Error(v) => ("Needs attention", &ui::BELL, k.c.danger.fg, v),
            Self::Empty(v) => ("Conversation", &crate::ORBIT, k.c.text_muted, v),
        }
    }
}

fn local_note(s: &Workspace, index: usize) -> Note {
    let l = live(s);
    let session = &l.app.sessions[index];
    if let Some(question) = crate::questions::preview(l, index) {
        return Note::Question(question);
    }
    if let Some(p) = l.app.approvals.iter().find(|p| {
        p.project_path == session.scope.project
            && p.provider == session.scope.provider.as_str()
            && (p.session_id.is_none() || p.session_id == session.remote_id)
    }) {
        return Note::Approval(excerpt(&p.summary));
    }
    if let Some(error) = session.error.as_deref().or_else(|| {
        session
            .conversation
            .failure
            .notice()
            .map(|n| n.message.as_str())
    }) {
        return Note::Error(excerpt(error));
    }
    if let Some(activity) = crate::activity::session_activity(s, index) {
        let task = (activity.kind
            == caelum_core::application::presentation::ActivityKind::Background)
            .then(|| {
                session
                    .conversation
                    .entries
                    .iter()
                    .rev()
                    .take_while(|e| e.kind() != EntryKind::User)
                    .find(|e| e.name == "subagent" && !e.complete())
                    .and_then(caelum_core::application::subagents::Subagent::from_entry)
            })
            .flatten();
        return Note::Working(task.map_or(activity.label, |t| excerpt(&t.description)));
    }
    let draft = l
        .drafts
        .get(index)
        .map_or(session.draft.as_str(), |d| &d.text);
    if !draft.trim().is_empty() {
        return Note::Draft(excerpt(draft));
    }
    if let Some(a) = session.attachments.first() {
        return Note::Draft(format!("Attachment · {}", excerpt(&a.path)));
    }
    if let Some((kind, shown)) = latest_message(&session.conversation.entries) {
        return if kind == EntryKind::User {
            Note::Prompt(shown)
        } else {
            Note::Reply(shown)
        };
    }
    Note::Empty(if session.remote_id.is_some() {
        "Open to read the conversation."
    } else {
        "Ready for a first message."
    })
}

fn vm_note(t: &caelum_core::remote::Thread) -> Note {
    let status = match t.turn {
        Turn::Idle => None,
        Turn::Submitting => Some("Sending the prompt…"),
        Turn::Queued(_) => Some("Queued on the VM…"),
        Turn::Following(_) => Some("Working on the VM…"),
        Turn::Finishing(_) => Some("Receiving final updates…"),
        Turn::Stopping(_) => Some("Stopping the turn…"),
    };
    if let Some(status) = status {
        return Note::Working(status.into());
    }
    if !t.draft.trim().is_empty() {
        return Note::Draft(excerpt(&t.draft));
    }
    for line in t.lines.iter().rev() {
        if line.speaker.is_assistant() {
            return Note::Reply(excerpt(&visible_text(&line.text)));
        }
        if line.speaker == Speaker::You {
            return Note::Prompt(excerpt(&line.text));
        }
    }
    if let Some(message) = t.continued.as_ref().and_then(|c| c.chat.messages.last()) {
        return match message {
            Message::User(v) => Note::Prompt(excerpt(v)),
            Message::Assistant(v) => Note::Reply(excerpt(&visible_text(v))),
        };
    }
    Note::Empty("Ready for a first message.")
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
    let name = checkouts::project_name(app, app.group_path(project));
    let identity = destination.map_or_else(|| name.to_owned(), |d| format!("{name} / {d}"));
    let ink = l.feed.project_ink(app, project, k);
    let (label, glyph, tone, snippet) = note.style(k);
    let owner = id.clone();
    k.interactive(
        col()
            .min_w(0.0)
            .grow(1.0)
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
