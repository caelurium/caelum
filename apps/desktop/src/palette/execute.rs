use caelum_core::application::{HistoryAction, Task};
use rune::{TextEdit, Tx};

use crate::{
    Workspace,
    live::{live, live_mut},
    panels::{self, Screen},
};

use super::Action;

pub(super) fn execute(s: &mut Workspace, tx: &mut Tx, action: Action) {
    live_mut(s).panels.set_overlay(None);
    match action {
        Action::NewSession => live_mut(s).new_session(),
        Action::NewGeneralSession => crate::live::new_general_session(s),
        Action::AttachToProject => {
            if let Some(i) = live(s).app.selected {
                live_mut(s).attach_menu = Some(i);
            }
        }
        Action::Launcher => panels::open_launcher(s),
        Action::Home => crate::home::open(s),
        Action::Overview => crate::overview::open(s, tx),
        Action::Settings(page) => {
            live_mut(s).panels.settings_page = page;
            panels::show(s, Screen::Settings);
        }
        Action::Worktrees => panels::show(s, Screen::Worktrees),
        Action::ToggleSidebar => {
            s.sidebar.toggle();
            live_mut(s).focus();
        }
        Action::ToggleInspector => {
            s.inspector.toggle();
            live_mut(s).focus();
        }
        Action::TogglePeacefulMode => {
            let l = live_mut(s);
            l.app.preferences.peaceful_mode = !l.app.preferences.peaceful_mode;
            l.focus();
        }
        Action::Files => panels::show(s, Screen::Files),
        Action::Changes => panels::show(s, Screen::Changes),
        Action::Runner => crate::runner::open(s),
        Action::SplitVertical => {
            if let Some(i) = live(s).app.selected {
                let target = (0..live(s).app.sessions.len())
                    .find(|&idx| idx != i)
                    .unwrap_or(i);
                crate::split::open_session(s, target, crate::split::SplitOrientation::Vertical);
            }
        }
        Action::SplitHorizontal => {
            if let Some(i) = live(s).app.selected {
                let target = (0..live(s).app.sessions.len())
                    .find(|&idx| idx != i)
                    .unwrap_or(i);
                crate::split::open_session(s, target, crate::split::SplitOrientation::Horizontal);
            }
        }
        Action::RotateSplit => crate::split::rotate_forward(s),
        Action::ToggleSplitOrientation => crate::split::toggle_orientation(s),
        Action::CloseSplit => crate::split::close(s),
        Action::OpenProject => crate::live::open_project(s, tx),
        Action::ChooseModel => crate::models::open(s, None),
        Action::Timeline => {
            if let Some(index) = live(s).app.selected {
                live_mut(s).commands.owner = Some(index);
                crate::commands::open_timeline(s);
            }
        }
        Action::Checkpoints => crate::commands::open_checkpoints(s),
        Action::Usage => {
            if let Some(index) = live(s).app.selected {
                let l = live_mut(s);
                l.commands.owner = Some(index);
                l.panels.set_overlay(Some(Screen::Usage));
                let scope = l.app.sessions[index].scope.clone();
                l.app.task(Task::Usage {
                    project: scope.project,
                    provider: scope.provider,
                });
            }
        }
        Action::Stats => crate::commands::open_stats(s),
        Action::ProviderSessions => {
            if let Some(index) = live(s).app.selected {
                let l = live_mut(s);
                let scope = l.app.sessions[index].scope.clone();
                l.commands.owner = Some(index);
                l.commands.sessions_search.clear();
                l.commands.sessions_selected = None;
                l.commands.sessions_search.focused = true;
                l.panels.set_overlay(Some(Screen::Sessions));
                l.app.discover_sessions(scope);
            }
        }
        Action::Fork => {
            if let Some(index) = live(s).app.selected {
                crate::commands::history(s, index, HistoryAction::Fork, None);
            }
        }
        Action::Rename => {
            if let Some(index) = live(s).app.selected {
                crate::commands::rename(s, index);
            }
        }
        Action::SideQuestion => {
            if let Some(index) = live(s).app.selected
                && live(s).app.sessions[index].remote_id.is_some()
            {
                let l = live_mut(s);
                l.app.dismiss_side_question(index);
                l.commands.side_owner = Some(index);
                l.commands.side_editor = TextEdit::new("");
                l.commands.side_editor.focused = true;
                l.drafts[index].focused = false;
            }
        }
        Action::SelectSession(index) => {
            crate::live::select_session(s, index);
            live_mut(s).focus();
        }
        Action::SelectProject(path) => crate::checkouts::select_project(s, &path),
        Action::SelectWorktree(path) => crate::checkouts::select(s, &path),
        Action::SplitProject(path) => {
            crate::split::open_project(s, &path, crate::split::SplitOrientation::Vertical);
        }
        Action::SplitWorktree(path) => {
            crate::split::open_checkout(s, &path, crate::split::SplitOrientation::Vertical);
        }
    }
}
