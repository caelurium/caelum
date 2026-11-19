use crate::{Workspace, live::live, quiet_button, theme, ui};
use caelum_core::application::presentation::Outcome;
use caelum_core::remote::{
    Checkout, Job, Line, Message, Phase, Speaker, Thread, ToolState, TurnEnd,
};
use rune::{El, ScrollSpec, col, icon, row, text};
use rune_kit::{Kit, Markdown, ScrollBar, glyphs};
use std::hash::{DefaultHasher, Hash, Hasher};

enum Entry<'a> {
    User(&'a str),
    Reply(&'a str, &'static str, bool),
    Line(&'a Line),
    Boundary,
}

fn entries(t: &Thread) -> &[Message] {
    t.continued
        .as_ref()
        .map_or(&[], |c| c.chat.messages.as_slice())
}

fn entry(t: &Thread, i: usize) -> Option<Entry<'_>> {
    let history = entries(t);
    if let Some(message) = history.get(i) {
        return Some(match message {
            Message::User(v) => Entry::User(v),
            Message::Assistant(v) => Entry::Reply(v, "Assistant · local history", false),
        });
    }
    if !history.is_empty() && i == history.len() {
        return Some(Entry::Boundary);
    }
    t.lines
        .get(i - history.len() - usize::from(!history.is_empty()))
        .map(Entry::Line)
}

pub(super) fn view(
    s: &Workspace,
    k: &Kit,
    id: &str,
    c: &Checkout,
    padding: f32,
    reduced: bool,
) -> El {
    let owner = c.thread.editor_key(id);
    let history = entries(&c.thread).len();
    let count = history + usize::from(history > 0) + c.thread.lines.len();
    let mut identities = DefaultHasher::new();
    owner.hash(&mut identities);
    history.hash(&mut identities);
    for line in &c.thread.lines {
        line.key.hash(&mut identities);
    }
    let following = matches!(c.phase, Phase::Ready { .. })
        && c.thread.turn.run().is_some()
        && matches!(
            live(s).app.remote.feeds.get(id),
            Some(Job::Busy(_) | Job::Wait(_))
        );
    let viewport = col()
        .key(format!("remote-transcript-{owner}"))
        .role("region")
        .label("Remote conversation")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec { stick_bottom: true });
    if count == 0 {
        if super::setup::Dock::for_checkout(c) == super::setup::Dock::Setup {
            return viewport;
        }
        return viewport.child(
            col()
                .w_full()
                .max_w(840.0)
                .px(padding)
                .py(36.0)
                .gap(10.0)
                .child(icon(&ui::SERVER, 22.0).color(k.c.icon_accent))
                .child(
                    text("Your remote workspace")
                        .font(rune::TextStyle::new(20.0, 550.0))
                        .color(k.c.text_base),
                )
                .child(ui::caption(
                    format!("Send a message to run {} on {}. The model is set in Boat → Agents & Proxying.", c.thread.provider.name(), c.title()),
                    k,
                )),
        );
    }
    let kit = k.clone();
    let keys = id.to_owned();
    let builders = keys.clone();
    let thread = owner.to_owned();
    let rows_owner = thread.clone();
    let mut viewport = viewport;
    let l = live(s);
    if l.remote
        .mounted
        .as_ref()
        .is_some_and(|(key, _)| key == owner)
        && let Some(e) = l.remote.editors.get(owner)
    {
        let position = e.reading.clone();
        viewport = viewport.on_scroll_offset(move |p| position.set(p));
    }
    viewport
        .child(
            col()
                .w_full()
                .max_w(840.0)
                .growth(rune::GrowthSpec::default())
                .virtual_rows(
                    count,
                    92.0,
                    move |s: &Workspace, i| {
                        if let Some(c) = live(s).app.remote.saved.checkouts.get(&keys)
                            && c.thread.editor_key(&keys) == thread
                            && let Some(Entry::Line(line)) = entry(&c.thread, i)
                            && let Some(id) = &line.key
                        {
                            return format!("remote-line-{thread}-{id}").into();
                        }
                        format!("remote-row-{thread}-{i}").into()
                    },
                    move |s: &Workspace, i| {
                        let Some(c) = live(s)
                            .app
                            .remote
                            .saved
                            .checkouts
                            .get(&builders)
                            .filter(|c| c.thread.editor_key(&builders) == rows_owner)
                        else {
                            return col();
                        };
                        let Some(e) = entry(&c.thread, i) else {
                            return col();
                        };
                        let answer = |l: &Line, who| {
                            reply(&l.text, who, l.partial && following, reduced, &kit)
                        };
                        let body = match e {
                            Entry::User(v) => user(v, &kit),
                            Entry::Reply(v, who, partial) => reply(v, who, partial, reduced, &kit),
                            Entry::Boundary => row()
                                .w_full()
                                .center_y()
                                .gap(10.0)
                                .child(ui::divider(&kit).min_w(0.0).grow(1.0))
                                .child(ui::caption("Continued on VM", &kit))
                                .child(ui::divider(&kit).min_w(0.0).grow(1.0)),
                            Entry::Line(l) => match l.speaker {
                                Speaker::You => user(&l.text, &kit),
                                Speaker::Codex => answer(l, "Codex"),
                                Speaker::OpenCode => answer(l, "OpenCode"),
                                Speaker::Tool(state) => tool(l, state, following, &kit),
                                Speaker::Outcome(end) => row()
                                    .w_full()
                                    .center_y()
                                    .gap(6.0)
                                    .child(ui::outcome(
                                        match end {
                                            TurnEnd::Finished => Outcome::Success,
                                            TurnEnd::Failed => Outcome::Failure,
                                            TurnEnd::Interrupted => Outcome::Interrupted,
                                            TurnEnd::Other => Outcome::Neutral,
                                        },
                                        &kit,
                                    ))
                                    .child(ui::caption(end.label(), &kit)),
                                Speaker::Activity => row()
                                    .w_full()
                                    .center_y()
                                    .gap(7.0)
                                    .child(icon(&ui::CLOCK, 12.0).color(kit.c.icon_muted))
                                    .child(ui::caption(&l.text, &kit).min_w(0.0).selectable(true)),
                            },
                        };
                        col().w_full().px(padding).py(9.0).child(body)
                    },
                )
                .virtual_rows_version(identities.finish()),
        )
        .scroll_bar(k)
}

fn user(message: &str, k: &Kit) -> El {
    row()
        .w_full()
        .justify_end()
        .child(ui::user_message(message, k))
}

fn reply(message: &str, who: &str, partial: bool, reduced: bool, k: &Kit) -> El {
    let copy = message.to_owned();
    col()
        .w_full()
        .gap(6.0)
        .child(
            row()
                .w_full()
                .center_y()
                .gap(7.0)
                .child(icon(&glyphs::SPARKLE, 13.0).color(k.c.icon_muted))
                .child(ui::caption(if partial { "Responding…" } else { who }, k).grow(1.0))
                .child(
                    quiet_button(&glyphs::COPY, "Copy response")
                        .on_press_tx(move |_: &mut Workspace, tx| tx.copy(copy.clone()))
                        .build(k),
                ),
        )
        .child(
            Markdown::new(message)
                .selectable(true)
                .streaming(partial)
                .reveal(if reduced { 0.0 } else { 0.12 })
                .render(&theme::markdown(k)),
        )
}

fn tool(line: &Line, state: ToolState, following: bool, k: &Kit) -> El {
    let name = line.text.as_str();
    let (outcome, label) = match state {
        ToolState::Running if !following => (Outcome::Neutral, "Last seen running"),
        ToolState::Running => (Outcome::Running, "Running"),
        ToolState::Complete => (Outcome::Success, "Completed"),
        ToolState::Failed => (Outcome::Failure, "Failed"),
        ToolState::Reported => (Outcome::Neutral, "Tool call"),
    };
    let glyph = match name.to_ascii_lowercase().as_str() {
        "bash" | "shell" => &ui::TERMINAL,
        "read" => &ui::FILE,
        "edit" | "write" | "apply_patch" => &ui::EDIT,
        "grep" | "glob" => &glyphs::SEARCH,
        _ => &ui::TOOL,
    };
    row()
        .w_full()
        .center_y()
        .gap(8.0)
        .px(10.0)
        .py(6.0)
        .radius(7.0)
        .fill(k.c.layer_01)
        .label(format!("{name} · {label}"))
        .child(icon(glyph, 14.0).color(k.c.icon_muted))
        .child(
            text(name)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .min_w(0.0)
                .truncate(),
        )
        .child(
            ui::caption(line.tool_detail.as_deref().unwrap_or(""), k)
                .min_w(0.0)
                .grow(1.0)
                .truncate(),
        )
        .child(ui::caption(label, k))
        .child(ui::outcome(outcome, k))
}
