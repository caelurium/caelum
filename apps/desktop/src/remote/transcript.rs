use crate::{Workspace, live::live, quiet_button, theme, ui};
use caelum_core::application::presentation::Outcome;
use caelum_core::remote::{
    Checkout, Job, Line, Message, Phase, Speaker, Thread, ToolState, Turn, TurnEnd,
};
use rune::{El, ScrollSpec, col, icon, row, text};
use rune_kit::{Kit, Markdown, ScrollBar, glyphs};
use std::{
    hash::{DefaultHasher, Hash, Hasher},
    rc::Rc,
};

enum Entry<'a> {
    User(&'a str),
    Reply(&'a str, &'static str, bool),
    Line(&'a Line),
    Boundary,
}

fn entries(t: &Thread) -> &[Message] {
    t.continuation().map_or(&[], |c| c.chat.messages.as_slice())
}

fn line_visible(line: &Line, peaceful: bool) -> bool {
    !peaceful || !matches!(line.speaker, Speaker::Tool(_) | Speaker::ReasoningSummary)
}

fn entry<'a>(t: &'a Thread, i: usize, visible_lines: &[usize]) -> Option<Entry<'a>> {
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
    visible_lines
        .get(i - history.len() - usize::from(!history.is_empty()))
        .and_then(|&index| t.lines.get(index))
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
    let peaceful = live(s).app.preferences.peaceful_mode;
    let owner = c.thread.editor_key(id);
    let history = entries(&c.thread).len();
    let visible_lines: Rc<Vec<usize>> = Rc::new(
        c.thread
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line_visible(line, peaceful))
            .map(|(i, _)| i)
            .collect(),
    );
    let count = history + usize::from(history > 0) + visible_lines.len();
    let mut identities = DefaultHasher::new();
    owner.hash(&mut identities);
    peaceful.hash(&mut identities);
    history.hash(&mut identities);
    for &index in visible_lines.iter() {
        c.thread.lines[index].key.hash(&mut identities);
    }
    let following = matches!(c.phase, Phase::Ready { .. })
        && c.thread.turn().run().is_some()
        && matches!(
            live(s).app.remote.feeds.get(id),
            Some(Job::Busy(_) | Job::Wait(_))
        );
    let pending = following
        .then(|| match c.thread.turn() {
            Turn::Queued(_) => Some("Queued on Boat…"),
            Turn::Stopping(_) => Some("Stopping turn…"),
            Turn::Finishing(_) => Some("Receiving final updates…"),
            Turn::Following(_)
                if !c
                    .thread
                    .lines
                    .iter()
                    .rev()
                    .take_while(|l| l.speaker != Speaker::You)
                    .any(|l| {
                        (!peaceful && l.speaker == Speaker::Tool(ToolState::Running))
                            || (l.partial && line_visible(l, peaceful))
                    }) =>
            {
                Some("Working on VM…")
            }
            _ => None,
        })
        .flatten();
    let clearance = crate::composer::clearance();
    pending.is_some().hash(&mut identities);
    (clearance.round() as u32).hash(&mut identities);
    let viewport = col()
        .key(format!("remote-transcript-{owner}"))
        .role("region")
        .label("Remote conversation")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .items_center()
        .scroll(ScrollSpec { stick_bottom: true });
    let l = live(s);
    if matches!(&l.remote.open_when_ready, Some((owner, super::checkout::Open::Native(caelum_core::remote::native::SessionIntent::New) | super::checkout::Open::Pi)) if owner == id)
    {
        return viewport;
    }
    if count == 0 && pending.is_none() {
        if super::setup::Dock::for_checkout(l, id, c) == super::setup::Dock::Setup {
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
                    format!("Choose a model or keep Boat default, then send your first message to {} on {}.", c.thread.provider.name(), c.title()),
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
    if l.remote
        .mounted
        .as_ref()
        .is_some_and(|(key, _)| key == owner)
        && let Some(e) = l.remote.editors.get(owner)
    {
        let position = e.reading.clone();
        viewport = viewport.on_scroll_offset(move |p| position.set(p));
    }
    let key_lines = visible_lines.clone();
    let row_lines = visible_lines;
    let tail = count + usize::from(pending.is_some());
    viewport
        .child(
            col()
                .w_full()
                .max_w(840.0)
                .growth(rune::GrowthSpec::default())
                .virtual_rows(
                    tail + 1,
                    92.0,
                    move |s: &Workspace, i| {
                        if i == tail {
                            return format!("remote-tail-{thread}").into();
                        }
                        if i == count {
                            return format!("remote-progress-{thread}").into();
                        }
                        if let Some(c) = live(s).app.remote.saved.checkouts.get(&keys)
                            && c.thread.editor_key(&keys) == thread
                            && let Some(Entry::Line(line)) = entry(&c.thread, i, &key_lines)
                            && let Some(id) = &line.key
                        {
                            return format!("remote-line-{thread}-{id}").into();
                        }
                        format!("remote-row-{thread}-{i}").into()
                    },
                    move |s: &Workspace, i| {
                        if i == tail {
                            return rune::el().w_full().h(clearance).a11y_hidden();
                        }
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
                        if i == count
                            && let Some(label) = pending
                        {
                            return row()
                                .w_full()
                                .px(padding)
                                .py(9.)
                                .gap(7.)
                                .center_y()
                                .role("status")
                                .label(label)
                                .child(icon(&ui::CLOCK, 14.).color(kit.c.icon_muted))
                                .child(ui::busy_text(label, kit.ty.small_medium, &kit));
                        }
                        let Some(e) = entry(&c.thread, i, &row_lines) else {
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
                                Speaker::Pi => answer(l, "Pi"),
                                Speaker::Antigravity => answer(l, "Antigravity"),
                                Speaker::OpenCode => answer(l, "OpenCode"),
                                Speaker::Tool(state) => activity::tool(
                                    s,
                                    &kit,
                                    &builders,
                                    &rows_owner,
                                    l,
                                    state,
                                    following,
                                ),
                                Speaker::ReasoningSummary => {
                                    activity::summary(s, &kit, &builders, &rows_owner, l, following)
                                }
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

mod activity;
