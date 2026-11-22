use crate::{
    Workspace,
    context::EditMenu,
    live::{live, live_mut},
    panels::Screen,
    ui::{self, Button},
};
use caelum_core::application::{ExportFormat, HistoryAction, SessionKey, commands::LocalCommand};
use rune::{El, FileDialogOptions, TextEdit, Tx, col, row};
use rune_kit::{ButtonVariant, Kit};

pub(super) struct RenameForm {
    owner: SessionKey,
    title: TextEdit,
}

pub(super) fn dispatch(
    s: &mut Workspace,
    tx: &mut Tx,
    index: usize,
    command: LocalCommand,
    args: &str,
) -> bool {
    let accepted = match command {
        LocalCommand::Fork | LocalCommand::Edit | LocalCommand::Rewind => {
            let action = match command {
                LocalCommand::Edit => HistoryAction::Edit,
                LocalCommand::Rewind => HistoryAction::Rewind,
                _ => HistoryAction::Fork,
            };
            history(s, index, action, None)
        }
        LocalCommand::Rename => {
            if args.is_empty() {
                rename(s, index)
            } else if let Some(owner) = owner(s, index) {
                live_mut(s).app.rename_session(index, &owner, args)
            } else {
                false
            }
        }
        LocalCommand::Export => {
            let format = match args {
                "" | "md" => ExportFormat::Markdown,
                "json" => ExportFormat::Json,
                _ => {
                    live_mut(s).app.sessions[index].error =
                        Some("Use /export md or /export json.".into());
                    return true;
                }
            };
            export(s, index, format, tx)
        }
        _ => return false,
    };
    if accepted {
        let l = live_mut(s);
        let draft = l.commands.previous_drafts[index].clone();
        l.drafts[index].replace_text(&draft);
        l.app.sessions[index].draft = draft;
    }
    true
}

fn owner(s: &mut Workspace, index: usize) -> Option<SessionKey> {
    let session = &mut live_mut(s).app.sessions[index];
    let key = session.key();
    if key.is_none() {
        session.error = Some("Send a message first to create a provider session.".into());
    }
    key
}

pub(crate) fn history(
    s: &mut Workspace,
    index: usize,
    action: HistoryAction,
    entry: Option<String>,
) -> bool {
    let l = live_mut(s);
    if !l.app.open_history_action(index, action, entry) {
        return false;
    }
    l.drafts[index].focused = false;
    l.commands.owner = Some(index);
    l.commands.search.clear();
    l.commands.search.focused = true;
    l.commands.fork_selected = None;
    l.panels.set_overlay(Some(Screen::Fork));
    true
}

pub(crate) fn export(s: &mut Workspace, index: usize, format: ExportFormat, tx: &mut Tx) -> bool {
    let Some(owner) = owner(s, index) else {
        return false;
    };
    tx.save_file(
        FileDialogOptions {
            title: format!("Export: {}", format.scope()),
            file_name: Some(format!("conversation.{}", format.extension())),
            ..Default::default()
        }
        .filter(
            match format {
                ExportFormat::Markdown => "Markdown",
                ExportFormat::Json => "JSON",
            },
            &[format.extension()],
        ),
        move |s: &mut Workspace, result, _| {
            let l = live_mut(s);
            if l.app.sessions[index].key().as_ref() != Some(&owner)
                || l.app.sessions[index].forgotten()
            {
                return;
            }
            match result {
                Ok(Some(path)) => {
                    l.app.export_session(index, &owner, format, path);
                }
                Ok(None) => {}
                Err(e) => l.app.sessions[index].error = Some(e.to_string()),
            }
        },
    );
    true
}

pub(crate) fn rename(s: &mut Workspace, index: usize) -> bool {
    let Some(owner) = owner(s, index) else {
        return false;
    };
    let l = live_mut(s);
    let mut title = TextEdit::new(&l.app.sessions[index].title);
    title.focused = true;
    title.set_selection(0, title.text.len());
    l.commands.rename = Some(RenameForm { owner, title });
    l.commands.owner = Some(index);
    l.panels.set_overlay(Some(Screen::Rename));
    true
}

fn submit(s: &mut Workspace, _: &mut Tx) {
    let l = live_mut(s);
    let (Some(index), Some(form)) = (l.commands.owner, l.commands.rename.as_ref()) else {
        return;
    };
    if l.app.rename_session(index, &form.owner, &form.title.text) {
        l.panels.set_overlay(None);
        l.commands.rename = None;
        l.focus();
    }
}

pub(super) fn rename_view(s: &Workspace, k: &Kit) -> El {
    let l = live(s);
    let Some(form) = &l.commands.rename else {
        return col();
    };
    let i = l.commands.owner.expect("rename owner");
    col()
        .w_full()
        .gap(12.0)
        .child(
            ui::field(&form.title, "Session title", "Session title", k)
                .single_line()
                .edit_menu("rename-title", |s| {
                    Some(&mut live_mut(s).commands.rename.as_mut()?.title)
                })
                .on_submit(submit),
        )
        .children(
            l.app.sessions[i]
                .error
                .as_ref()
                .map(|e| ui::caption(e, k).color(k.c.danger.fg).wrap()),
        )
        .child(ui::divider(k))
        .child(
            row().w_full().justify_end().child(
                Button::new("Save title")
                    .variant(ButtonVariant::Contrast)
                    .disabled(form.title.text.trim().is_empty())
                    .on_press_tx(submit)
                    .build(k)
                    .key("rename-save"),
            ),
        )
}
