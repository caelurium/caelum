use crate::{
    Workspace,
    live::{live, live_mut},
    theme, ui,
};
use caelum_core::{
    application::presentation::Outcome,
    remote::{Line, ToolLoad, ToolOutput, ToolState},
};
use rune::{El, ScrollSpec, col, icon, row, text};
use rune_kit::{Kit, Markdown, ScrollBar, glyphs};

pub(super) fn tool(
    s: &Workspace,
    k: &Kit,
    id: &str,
    owner: &str,
    line: &Line,
    state: ToolState,
    following: bool,
) -> El {
    let key = line.key.as_deref().unwrap_or("");
    let panel = live(s)
        .app
        .remote
        .tool
        .as_ref()
        .filter(|p| p.owns(id, owner, key));
    let state = match panel.map(|p| &p.load) {
        Some(ToolLoad::Ready(body))
            if matches!(body.state, ToolState::Complete | ToolState::Failed) =>
        {
            body.state
        }
        _ => state,
    };
    let (outcome, label) = match state {
        ToolState::Running if !following => (Outcome::Neutral, "Last seen running"),
        ToolState::Running => (Outcome::Running, "Running"),
        ToolState::Complete => (Outcome::Success, "Completed"),
        ToolState::Failed => (Outcome::Failure, "Failed"),
        ToolState::Reported => (Outcome::Neutral, "Tool call"),
    };
    let name = &line.text;
    let glyph = match name.to_ascii_lowercase().as_str() {
        "bash" | "shell" | "exec_command" | "commandexecution" => &ui::TERMINAL,
        "read" | "read_file" => &ui::FILE,
        "edit" | "write" | "apply_patch" => &ui::EDIT,
        "grep" | "glob" | "search" => &glyphs::SEARCH,
        _ => &ui::TOOL,
    };
    let mut header = row()
        .w_full()
        .center_y()
        .gap(7.)
        .px(6.)
        .py(7.)
        .radius(7.)
        .label(format!("{name} · {label}"))
        .child(icon(glyph, 15.).color(k.c.icon_muted))
        .child(if outcome == Outcome::Running {
            ui::busy_text(name, k.ty.small_medium, k)
        } else {
            text(name).font(k.ty.small_medium).color(k.c.text_base)
        })
        .child(
            ui::caption(line.tool_detail.as_deref().unwrap_or(""), k)
                .truncate()
                .min_w(0.)
                .grow(1.),
        )
        .child(ui::caption(label, k))
        .child(ui::outcome(outcome, k));
    if key.is_empty() || line.tool_source.is_none() {
        return header
            .role("group")
            .help("Only the summary is available for this saved call.");
    }
    let (id, owner, line_key) = (id.to_owned(), owner.to_owned(), key.to_owned());
    let element = format!("vm-tool-{owner}-{key}");
    header = k
        .interactive(header.child(ui::disclosure(panel.is_some(), k)))
        .role("button")
        .focusable(true)
        .focus_ring(1.5, 0., k.c.border_focus)
        .expanded(panel.is_some())
        .key(&element)
        .on_press_tx(move |s: &mut Workspace, tx| {
            tx.keep_on_screen(&element);
            live_mut(s).app.toggle_vm_tool(&id, &owner, &line_key);
        });
    col()
        .w_full()
        .gap(5.)
        .child(header)
        .children(panel.map(|p| {
            match &p.load {
                ToolLoad::Loading(_) => ui::caption("Loading tool details…", k)
                    .pl(27.)
                    .role("status"),
                ToolLoad::Failed(e) => ui::caption(format!("{e} Close and reopen to retry."), k)
                    .color(k.c.danger.fg)
                    .wrap()
                    .pl(27.),
                ToolLoad::Ready(body) => details(body, k, following),
            }
        }))
}

fn details(body: &ToolOutput, k: &Kit, following: bool) -> El {
    let mut content = col().w_full().pl(27.).gap(6.);
    if !body.command.is_empty() {
        content = content.child(ui::caption("Command", k)).child(
            col()
                .w_full()
                .max_h(100.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text(&body.command)
                        .font(k.ty.mono)
                        .color(k.c.text_base)
                        .wrap_anywhere()
                        .selectable(true)
                        .w_full(),
                )
                .scroll_bar(k),
        );
    }
    if body.output.is_empty() {
        let message = if following && body.state == ToolState::Running {
            "Waiting for tool result…"
        } else {
            "No text output reported."
        };
        content = content.child(ui::caption(message, k));
    } else {
        let error = body.state == ToolState::Failed;
        let copy = body.output.clone();
        content = content
            .child(
                row()
                    .w_full()
                    .center_y()
                    .child(ui::caption(if error { "Error" } else { "Output" }, k).grow(1.))
                    .child(
                        crate::quiet_button(&glyphs::COPY, "Copy displayed output")
                            .on_press_tx(move |_: &mut Workspace, tx| tx.copy(copy.clone()))
                            .build(k),
                    ),
            )
            .child(crate::activity::output_rail(
                col()
                    .w_full()
                    .max_h(220.)
                    .scroll(ScrollSpec {
                        stick_bottom: false,
                    })
                    .child(
                        text(&body.output)
                            .font(crate::activity::output_font(k))
                            .color(k.c.text_base)
                            .wrap_anywhere()
                            .selectable(true)
                            .w_full(),
                    )
                    .scroll_bar(k),
                if error {
                    k.c.danger.fg
                } else {
                    k.c.border_muted
                },
            ));
    }
    content.children(body.clipped.then(|| ui::caption("Preview truncated", k)))
}

pub(super) fn summary(
    s: &Workspace,
    k: &Kit,
    id: &str,
    owner: &str,
    line: &Line,
    following: bool,
) -> El {
    let label = if line.partial && following {
        "Thinking…"
    } else if line.text.is_empty() {
        "Reasoning activity"
    } else {
        "Reasoning summary"
    };
    let key = line.key.as_deref().unwrap_or("");
    let open = live(s)
        .remote
        .editors
        .get(owner)
        .and_then(|e| e.summary.as_deref())
        == Some(key);
    let mut header = row()
        .w_full()
        .gap(7.)
        .center_y()
        .px(6.)
        .py(7.)
        .radius(7.)
        .label(label)
        .child(icon(&ui::BRAIN, 15.).color(k.c.icon_muted))
        .child(if line.partial && following {
            ui::busy_text(label, k.ty.small_medium, k)
        } else {
            text(label).font(k.ty.small_medium).color(k.c.text_base)
        });
    if line.text.is_empty() || key.is_empty() {
        return header.role("group");
    }
    header = header
        .child(
            ui::caption(
                if open {
                    ""
                } else {
                    line.text.lines().next().unwrap_or("")
                },
                k,
            )
            .min_w(0.)
            .grow(1.)
            .truncate(),
        )
        .child(ui::disclosure(open, k));
    let element = format!("vm-summary-{owner}-{key}");
    let (checkout, thread, key) = (id.to_owned(), owner.to_owned(), key.to_owned());
    let header = k
        .interactive(header)
        .role("button")
        .focusable(true)
        .expanded(open)
        .focus_ring(1.5, 0., k.c.border_focus)
        .key(&element)
        .on_press_tx(move |s: &mut Workspace, tx| {
            let l = live_mut(s);
            if l.app.remote.saved.selected.as_ref() == Some(&checkout)
                && l.app.remote.saved.checkouts[&checkout]
                    .thread
                    .editor_key(&checkout)
                    == thread
                && let Some(editor) = l.remote.editors.get_mut(&thread)
            {
                editor.summary = (editor.summary.as_ref() != Some(&key)).then(|| key.clone());
                tx.keep_on_screen(&element);
            }
        });
    col().w_full().gap(5.).child(header).children(open.then(|| {
        Markdown::new(&line.text)
            .selectable(true)
            .render(&theme::markdown(k))
            .pl(27.)
    }))
}
