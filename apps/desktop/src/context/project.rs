use super::{Item, Menu, show};
use crate::{
    Workspace,
    live::{live, live_mut},
    ui,
};
use caelum_core::application::{ProjectMove, Task};
use rune::El;
use rune_kit::glyphs;

pub(crate) fn project(el: El, project: String) -> El {
    el.on_secondary_press(move |s: &mut Workspace, at, _| {
        let project = project.clone();
        let origin = crate::code_view::source_key(&project, "project");
        let app = &live(s).app;
        let menu = Menu::new()
            .item(Item::new("new", "New session").icon(&crate::PLUS))
            .item(Item::new("worktree", "Create worktree…").icon(&ui::BRANCH))
            .item(Item::new("remote", "Open on remote machine…").icon(&crate::ORBIT))
            .separator()
            .item(
                Item::new("up", "Move project up")
                    .icon(&glyphs::CHEVRON_UP)
                    .enabled(app.can_move_project(&project, ProjectMove::Up)),
            )
            .item(
                Item::new("down", "Move project down")
                    .icon(&glyphs::CHEVRON_DOWN)
                    .enabled(app.can_move_project(&project, ProjectMove::Down)),
            )
            .separator()
            .item(Item::new("copy", "Copy project path").icon(&glyphs::COPY))
            .item(Item::new("reveal", "Reveal in Finder").icon(&ui::FOLDER))
            .separator()
            .item(Item::new("close", "Close project").icon(&glyphs::CLOSE));
        show(s, at, origin.clone(), menu, move |s, pick, tx| match pick {
            "up" | "down" => {
                let direction = if pick == "up" {
                    ProjectMove::Up
                } else {
                    ProjectMove::Down
                };
                live_mut(s).app.move_project(&project, direction);
                tx.scroll_into_view(&origin);
            }
            "copy" => tx.copy(project.clone()),
            "remote" => crate::remote::open(s, project.clone()),
            "close" => live_mut(s).close_project(&project),
            "reveal" => live_mut(s).app.task(Task::Reveal {
                project: project.clone(),
                path: ".".into(),
            }),
            "new" => {
                let l = live_mut(s);
                l.app.select_project(&project);
                l.new_session();
            }
            "worktree" => {
                let provider = live(s).app.preferences.default_provider;
                crate::worktree::open(s, project.clone(), provider);
            }
            _ => {}
        });
    })
}
