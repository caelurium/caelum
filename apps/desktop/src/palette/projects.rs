use caelum_core::application::Workspace;

use super::{Action, BadgeTone, Entry, Section};

pub(super) fn append_entries(app: &Workspace, entries: &mut Vec<Entry>) {
    for project in &app.projects {
        if app.group_path(&project.path) != project.path {
            continue;
        }
        let is_active = app.active_project.as_deref() == Some(project.path.as_str());
        entries.push(Entry::new(
            Section::Projects,
            &project.name,
            &project.path,
            None,
            is_active.then_some(("Active", BadgeTone::Muted)),
            "project repository",
            Action::SelectProject(project.path.clone()),
        ));
        entries.push(Entry::new(
            Section::Projects,
            format!("Split: {}", project.name),
            format!("Open {} in a split pane", project.name),
            None,
            None,
            &format!("{} split pane project side by side", project.path),
            Action::SplitProject(project.path.clone()),
        ));
        for tree in app.managed_worktrees(&project.path) {
            let tree_name = tree.path.rsplit('/').next().unwrap_or(&tree.path);
            let branch = tree.branch.as_deref().unwrap_or("detached");
            let is_active_tree = app.active_project.as_deref() == Some(tree.path.as_str());
            entries.push(Entry::new(
                Section::Projects,
                format!("{} / {}", project.name, tree_name),
                format!("Worktree · {branch}"),
                None,
                is_active_tree.then_some(("Active", BadgeTone::Muted)),
                &format!("{} {} worktree", tree.path, branch),
                Action::SelectWorktree(tree.path.clone()),
            ));
            entries.push(Entry::new(
                Section::Projects,
                format!("Split: {} / {}", project.name, tree_name),
                format!("Open worktree ({branch}) in a split pane"),
                None,
                None,
                &format!("{} {} split pane worktree side by side", tree.path, branch),
                Action::SplitWorktree(tree.path.clone()),
            ));
        }
    }
}
