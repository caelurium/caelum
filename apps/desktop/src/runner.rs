//! Checkout command runner overlay: run, stop, rerun, and attach output to the active draft.

use caelum_core::{
    application::presentation::{duration, now},
    runner::RunState,
};
use rune::{El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, row, text};
use rune_kit::{ButtonVariant, Kit, ScrollBar, glyphs};

use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    panels::Screen,
    quiet_button,
    ui::{self, Button},
};

pub struct State {
    pub command: TextEdit,
    pub project: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            command: TextEdit::new(""),
            project: None,
        }
    }
}

pub fn active_project(s: &Workspace) -> Option<String> {
    let l = live(s);
    l.app
        .selected
        .map(|i| l.app.sessions[i].scope.project.clone())
        .filter(|p| caelum_core::remote::native::checkout_id(p).is_none())
        .or_else(|| {
            l.app
                .active_project
                .clone()
                .filter(|p| caelum_core::remote::native::checkout_id(p).is_none())
        })
        .or_else(|| {
            l.app
                .projects
                .iter()
                .find(|p| caelum_core::remote::native::checkout_id(&p.path).is_none())
                .map(|p| p.path.clone())
        })
}

pub fn open(s: &mut Workspace) {
    let Some(project) = active_project(s) else {
        return;
    };
    open_for(s, project);
}

pub fn open_for(s: &mut Workspace, project: String) {
    let l = live_mut(s);
    l.runner.project = Some(project.clone());
    l.app.refresh_project_config(&project);
    if l.runner.command.text.trim().is_empty()
        && let Some(last) = l
            .app
            .runner
            .get(&project)
            .and_then(|r| r.history.first())
            .cloned()
    {
        l.runner.command.replace_text(last);
    }
    l.runner.command.focused = true;
    l.panels.set_overlay(Some(Screen::Runner));
}

pub fn run(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(project) = l.runner.project.clone() else {
        return;
    };
    let command = l.runner.command.text.clone();
    l.app.run_checkout_command(project, command);
}

pub fn stop(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(project) = l.runner.project.clone() else {
        return;
    };
    l.app.stop_checkout_command(&project);
}

pub fn rerun(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(project) = l.runner.project.clone() else {
        return;
    };
    let command = l
        .app
        .runner
        .get(&project)
        .and_then(|r| {
            r.state
                .command()
                .map(str::to_owned)
                .or_else(|| r.history.first().cloned())
        })
        .unwrap_or_else(|| l.runner.command.text.clone());
    if !command.trim().is_empty() {
        l.runner.command.replace_text(command.clone());
        l.app.run_checkout_command(project, command);
    }
}

pub fn attach(s: &mut Workspace) {
    let l = live_mut(s);
    let Some(project) = l.runner.project.clone() else {
        return;
    };
    let Some(index) = l.app.selected else {
        return;
    };
    if l.app.attach_runner_output(index, &project) {
        l.panels.set_overlay(None);
        l.focus();
    }
}

pub fn on_key(s: &mut Workspace, ev: &KeyEvent, _tx: &mut Tx) -> bool {
    if live(s).panels.overlay() != Some(Screen::Runner) {
        return false;
    }
    if ev.key == Key::Enter && !ev.mods.shift {
        run(s);
        return true;
    }
    false
}

pub fn panel(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some(project) = l.runner.project.as_deref() else {
        return ui::empty(
            "No local checkout selected",
            "Open a project to run commands.",
            k,
        );
    };
    let checkout = l.app.runner.get(project);
    let state = checkout.map(|c| &c.state).unwrap_or(&RunState::Idle);
    let running = state.running();
    let has_output = state.output().is_some();
    let can_attach = has_output && l.app.selected.is_some();
    let can_rerun =
        !running && (state.command().is_some() || checkout.is_some_and(|c| !c.history.is_empty()));

    let input = ui::field(
        &l.runner.command,
        "Checkout command",
        "e.g. cargo test, make check…",
        k,
    )
    .grow(1.0)
    .min_w(0.0)
    .edit_menu("runner-command", |s| Some(&mut live_mut(s).runner.command))
    .on_input_key(on_key);

    let action_btn = if running {
        Button::new("Stop")
            .variant(ButtonVariant::Danger)
            .on_press(stop)
            .build(k)
            .key("runner-stop")
    } else {
        Button::new("Run")
            .variant(ButtonVariant::Contrast)
            .disabled(l.runner.command.text.trim().is_empty())
            .on_press(run)
            .build(k)
            .key("runner-run")
    };

    let controls = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(input)
        .child(action_btn)
        .child(
            Button::new("Rerun")
                .variant(ButtonVariant::Outline)
                .disabled(!can_rerun)
                .on_press(rerun)
                .build(k)
                .key("runner-rerun"),
        );

    let mut body = col()
        .key("runner-panel")
        .w_full()
        .gap(12.0)
        .child(ui::caption(project, k).truncate())
        .child(controls);

    if let Some(error) = l.app.project_config_error(project) {
        body = body.child(
            text(error)
                .font(k.ty.small)
                .color(k.c.warning.fg)
                .wrap()
                .key("runner-config-error"),
        );
    }

    if let Some(presets) = l
        .app
        .project_config(project)
        .map(|c| c.runner_presets.as_slice())
        .filter(|p| !p.is_empty())
    {
        let mut chips = row().w_full().gap(6.0).wrap().center_y();
        chips = chips.child(ui::caption("Presets", k));
        for (i, preset) in presets.iter().enumerate() {
            let cmd_run = preset.command.clone();
            chips = chips.child(
                Button::new(preset.name.clone())
                    .variant(ButtonVariant::Outline)
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        l.runner.command.replace_text(cmd_run.clone());
                        run(s);
                    })
                    .build(k)
                    .help(preset.command.clone())
                    .key(format!("runner-preset-{i}")),
            );
        }
        body = body.child(chips);
    }

    if let Some(history) = checkout.map(|c| &c.history).filter(|h| !h.is_empty()) {
        let mut chips = row().w_full().gap(6.0).wrap();
        for (i, cmd) in history.iter().take(6).enumerate() {
            let cmd_run = cmd.clone();
            chips = chips.child(
                Button::new(cmd.clone())
                    .variant(ButtonVariant::GhostMuted)
                    .on_press(move |s: &mut Workspace| {
                        let l = live_mut(s);
                        l.runner.command.replace_text(cmd_run.clone());
                        run(s);
                    })
                    .build(k)
                    .key(format!("runner-history-{i}")),
            );
        }
        body = body.child(chips);
    }

    let (status_label, status_color, elapsed) = match state {
        RunState::Idle => ("Idle".to_owned(), k.c.text_muted, None),
        RunState::Running { started_at, .. } => (
            "Running".to_owned(),
            k.c.text_accent,
            Some(duration(now().saturating_sub(*started_at))),
        ),
        RunState::Exited {
            code: Some(0),
            duration_ms,
            ..
        } => (
            "Exit 0".to_owned(),
            k.c.success.fg,
            Some(duration(*duration_ms)),
        ),
        RunState::Exited {
            code: Some(c),
            duration_ms,
            ..
        } => (
            format!("Exit {c}"),
            k.c.danger.fg,
            Some(duration(*duration_ms)),
        ),
        RunState::Exited {
            code: None,
            duration_ms,
            ..
        } => (
            "Exited".to_owned(),
            k.c.text_muted,
            Some(duration(*duration_ms)),
        ),
        RunState::Cancelled { duration_ms, .. } => (
            "Stopped".to_owned(),
            k.c.warning.fg,
            Some(duration(*duration_ms)),
        ),
        RunState::Failed { .. } => ("Failed".to_owned(), k.c.danger.fg, None),
    };

    let copy_text = state.output().map(|(o, _)| o.to_owned());
    let header = row()
        .w_full()
        .center_y()
        .gap(8.0)
        .child(
            text(status_label)
                .font(k.ty.small_medium)
                .color(status_color)
                .px(6.0)
                .py(2.0)
                .radius(5.0)
                .fill(k.c.layer_02),
        )
        .children(elapsed.map(|d| ui::caption(d, k)))
        .child(rune::el().grow(1.0))
        .children(copy_text.map(|copy| {
            quiet_button(&glyphs::COPY, "Copy output")
                .on_press_tx(move |_: &mut Workspace, tx| tx.copy(copy.clone()))
                .build(k)
                .key("runner-copy")
        }))
        .child(
            Button::new("Attach output to prompt")
                .variant(ButtonVariant::Outline)
                .disabled(!can_attach)
                .on_press(attach)
                .build(k)
                .key("runner-attach"),
        );

    body = body.child(header);

    match state {
        RunState::Idle => {
            body = body.child(ui::empty(
                "No command run yet",
                "Enter a shell command above to run it in this checkout.",
                k,
            ));
        }
        RunState::Failed { error, .. } => {
            body = body.child(
                ui::card(k).child(
                    text(error.clone())
                        .font(k.ty.mono)
                        .color(k.c.danger.fg)
                        .wrap()
                        .selectable(true),
                ),
            );
        }
        RunState::Running {
            output, truncated, ..
        }
        | RunState::Exited {
            output, truncated, ..
        }
        | RunState::Cancelled {
            output, truncated, ..
        } => {
            let mut card = col()
                .w_full()
                .h(300.0)
                .padding(12.0)
                .gap(6.0)
                .radius(8.0)
                .fill(k.c.layer_01)
                .border(1.0, k.c.border_muted);
            if *truncated {
                card = card.child(ui::caption("Showing the last 64 KiB of command output.", k));
            }
            let display = if output.is_empty() {
                if running {
                    "Waiting for output…".to_owned()
                } else {
                    "(no output)".to_owned()
                }
            } else {
                output.clone()
            };
            card = card.child(
                col()
                    .key("runner-output")
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .scroll(ScrollSpec { stick_bottom: true })
                    .child(
                        text(display)
                            .font(k.ty.mono)
                            .color(k.c.text_base)
                            .wrap()
                            .selectable(true),
                    )
                    .scroll_bar(k),
            );
            body = body.child(card);
        }
    }

    body
}
