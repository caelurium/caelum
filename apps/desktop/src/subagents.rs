//! Named, session-linked tasks using the transcript's existing disclosure state.
use crate::{
    Workspace,
    live::{live, live_mut},
    panels::Screen,
    quiet_button, ui,
};
use caelum_core::application::{
    Entry,
    presentation::{Outcome, Tool, now},
    subagents::Subagent,
};
use rune::{El, col, icon, row, text};
use rune_kit::{ButtonVariant, Kit, ScrollBar, glyphs};

pub fn view(s: &Workspace, k: &Kit, index: usize, entry: &Entry, task: &Subagent) -> El {
    let tool = Tool::from_entry(entry, now());
    let id = entry.id.clone();
    let expanded = live(s).expanded.contains(&(index, id.clone()));
    let state = match tool.outcome {
        Outcome::Running => "Running",
        Outcome::Success => "Completed",
        Outcome::Failure => "Failed",
        Outcome::Interrupted => "Cancelled",
        _ => "Finished",
    };
    let ink = match tool.outcome {
        Outcome::Failure => k.c.danger.fg,
        Outcome::Interrupted => k.c.warning.fg,
        _ => k.c.text_muted,
    };
    let heading = k
        .interactive(
            col()
                .w_full()
                .gap(6.0)
                .padding(10.0)
                .radius(8.0)
                .child(
                    row()
                        .w_full()
                        .center_y()
                        .gap(7.0)
                        .child(icon(&ui::AGENT, 15.0).color(k.c.icon_muted))
                        .child(
                            text(format!("Subagent · {}", task.agent))
                                .font(k.ty.small_medium)
                                .color(k.c.text_base)
                                .truncate()
                                .min_w(0.0)
                                .grow(1.0),
                        )
                        .child(ui::caption(state, k).color(ink))
                        .children(
                            (tool.outcome != Outcome::Running)
                                .then(|| ui::outcome(tool.outcome, k)),
                        )
                        .child(ui::disclosure(expanded, k)),
                )
                .child(
                    text(&task.description)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .wrap_anywhere(),
                ),
        )
        .role("button")
        .label(format!("{}: {}", task.agent, task.description))
        .description(state)
        .expanded(expanded)
        .focusable(true)
        .focus_ring(1.5, 0.0, k.c.border_focus)
        .key(format!("subagent-{index}-{}", entry.id))
        .on_press_tx(move |s: &mut Workspace, tx| {
            tx.keep_on_screen(format!("subagent-{index}-{id}"));
            let expanded = &mut live_mut(s).expanded;
            if !expanded.remove(&(index, id.clone())) {
                expanded.insert((index, id.clone()));
            }
        });
    let mut content = col()
        .w_full()
        .radius(9.0)
        .border(1.0, k.c.border_muted)
        .child(heading);
    let mut footer = row().w_full().center_y().gap(6.0).px(10.0).pb(6.0).child(
        ui::caption(
            if tool.outcome == Outcome::Running {
                if task.session_id.is_some() {
                    "Working in a child session"
                } else {
                    "Starting task…"
                }
            } else {
                &tool.timing
            },
            k,
        )
        .grow(1.0),
    );
    if task.session_id.is_some() {
        let id = entry.id.clone();
        footer = footer.child(
            ui::Button::new("Open session")
                .variant(ButtonVariant::GhostMuted)
                .on_press(move |s: &mut Workspace| {
                    let l = live_mut(s);
                    if l.app.open_subagent(index, &id).is_some() {
                        l.sync_editors();
                        l.panels.screen = Screen::Conversation;
                        l.panels.overlay = None;
                    }
                })
                .build(k)
                .key(format!("subagent-open-{index}-{}", entry.id)),
        );
    }
    content = content.child(footer);
    if !tool.error.is_empty() {
        content = content.child(
            ui::caption(&tool.error, k)
                .color(k.c.danger.fg)
                .px(10.0)
                .pb(8.0),
        );
    }
    if expanded {
        let mut details = col().w_full().gap(10.0).padding(10.0);
        if let Some(model) = &task.model {
            details = details.child(ui::caption(format!("Model · {model}"), k));
        }
        for (name, value) in [("Result", &tool.output), ("Prompt", &task.prompt)] {
            if !value.is_empty() {
                details = details.child(section(s, k, index, &entry.id, name, value));
            }
        }
        content = content.child(details);
    }
    content
}

fn section(s: &Workspace, k: &Kit, index: usize, id: &str, name: &str, value: &str) -> El {
    let key = format!("subagent-{name}-{index}-{id}");
    let expanded_id = format!("{id}:subagent-{name}");
    let full = live(s).expanded.contains(&(index, expanded_id.clone()));
    let cutoff = value.char_indices().nth(12_000).map(|(i, _)| i);
    let preview = cutoff.filter(|_| !full).map_or(value, |end| &value[..end]);
    let copy = value.to_owned();
    let mut body = col()
        .w_full()
        .gap(4.0)
        .child(
            row()
                .w_full()
                .center_y()
                .child(ui::caption(name, k).grow(1.0))
                .child(
                    quiet_button(&glyphs::COPY, &format!("Copy {}", name.to_lowercase()))
                        .on_press_tx(move |_: &mut Workspace, tx| tx.copy(copy.clone()))
                        .build(k)
                        .key(format!("{key}-copy")),
                ),
        )
        .child(
            col()
                .w_full()
                .max_h(220.0)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text(preview)
                        .font(k.ty.small)
                        .color(k.c.text_base)
                        .wrap_anywhere()
                        .selectable(true)
                        .w_full(),
                )
                .scroll_bar(k)
                .key(format!("{key}-scroll")),
        );
    if cutoff.is_some() {
        body = body.child(
            ui::Button::new(if full { "Show less" } else { "Show full text" })
                .variant(ButtonVariant::GhostMuted)
                .on_press_tx(move |s: &mut Workspace, tx| {
                    tx.keep_on_screen(format!("{key}-scroll"));
                    let expanded = &mut live_mut(s).expanded;
                    if !expanded.remove(&(index, expanded_id.clone())) {
                        expanded.insert((index, expanded_id.clone()));
                    }
                })
                .build(k)
                .self_start(),
        );
    }
    body
}

pub fn parent(s: &Workspace, k: &Kit, index: usize) -> Option<El> {
    let parent = live(s).app.subagent_parent(index)?;
    Some(
        ui::Button::new("Back to parent session")
            .variant(ButtonVariant::GhostMuted)
            .on_press(move |s: &mut Workspace| live_mut(s).app.select(parent))
            .build(k)
            .help(&live(s).app.sessions[parent].title)
            .key("subagent-parent")
            .self_start(),
    )
}

pub(crate) fn scene(s: &mut Workspace, status: &str) {
    use caelum_core::application::{Conversation, Provider};
    use serde_json::json;

    let l = live_mut(s);
    let session = &mut l.app.sessions[0];
    session.scope.provider = Provider::OpenCode;
    session.remote_id = Some("ses_parent".into());
    session.conversation = Conversation::default();
    let start = now().saturating_sub(12_000);
    let mut messages = vec![
        json!({"id":"msg_prompt", "type":"user", "text":"Map the request flow and review the retry behaviour."}),
        json!({
        "id":"msg_tasks", "type":"assistant", "time":{"created":start,"completed":start+100}, "content":[
            {"id":"explore", "type":"tool", "name":"subagent", "time":{"created":start,"completed":start+100}, "state":{
                "status":"completed", "input":{"agent":"explore", "description":"Map request flow", "prompt":"Trace the request from the router to the persistence layer. Identify the owning modules and report the relevant entry points.", "background":true},
                "metadata":{"sessionID":"ses_explore", "status":"running"}, "content":[{"type":"text","text":"Started"}]}}
        ]}),
    ];
    if status != "running" {
        messages.push(json!({"id":"msg_report", "type":"synthetic", "time":{"created":start+9000}, "description":"Map request flow",
            "metadata":{"source":"subagent", "childID":"ses_explore", "agent":"explore", "state":status},
            "text": if status == "error" { "The review could not finish: the service was unavailable." } else { "Requests enter through the router, pass through validation, and reach the persistence worker.\n\nThe session owns the request generation, so late replies cannot overwrite newer work." }}));
    }
    session
        .conversation
        .history(Provider::OpenCode, &messages, false);
    l.expanded.insert((0, "opencode-tool-explore".into()));
}
