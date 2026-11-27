use super::*;
use std::process::Command;

fn init_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    run(&["init"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    fs::write(dir.path().join("README.md"), "initial\n").unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.log\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-m", "initial"]);
    dir
}

#[test]
fn captures_diffs_and_restores_modified_deleted_and_untracked_files_without_touching_index() {
    let dir = init_git_repo();
    let root = dir.path().to_str().unwrap();

    // Stage an unrelated edit in the real index before capturing.
    fs::write(dir.path().join("staged.txt"), "staged-before\n").unwrap();
    assert!(Command::new("git")
        .current_dir(dir.path())
        .args(["add", "staged.txt"])
        .status()
        .unwrap()
        .success());
    // Also have a pre-existing untracked file and an ignored file.
    fs::write(dir.path().join("draft.txt"), "before-turn\n").unwrap();
    fs::write(dir.path().join("ignored.log"), "keep-me\n").unwrap();

    let cp = capture(root, "Refactor parser\nsecond line")
        .unwrap()
        .expect("checkpoint captured");
    assert_eq!(cp.prompt_excerpt, "Refactor parser");
    assert!(cp.ref_name.starts_with("refs/caelum/checkpoints/"));

    // Simulate agent turn edits: modify tracked, modify untracked, delete tracked, create new file.
    fs::write(dir.path().join("README.md"), "edited-by-agent\n").unwrap();
    fs::write(dir.path().join("draft.txt"), "edited-draft\n").unwrap();
    fs::write(dir.path().join("new_dir_file.txt"), "brand-new\n").unwrap();
    fs::write(dir.path().join("ignored.log"), "still-ignored\n").unwrap();

    let diff = diff_since(root, &cp.tree_oid).unwrap();
    assert_eq!(
        diff.paths,
        vec![
            "README.md".to_owned(),
            "draft.txt".to_owned(),
            "new_dir_file.txt".to_owned(),
        ]
    );
    assert!(diff.diff.contains("edited-by-agent"));
    assert!(!diff.truncated);

    // Restore to the checkpoint.
    let restored = restore(root, &cp.tree_oid).unwrap();
    assert_eq!(restored.len(), 3);
    assert_eq!(
        fs::read_to_string(dir.path().join("README.md")).unwrap(),
        "initial\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("draft.txt")).unwrap(),
        "before-turn\n"
    );
    assert!(!dir.path().join("new_dir_file.txt").exists());
    // Ignored file is untouched.
    assert_eq!(
        fs::read_to_string(dir.path().join("ignored.log")).unwrap(),
        "still-ignored\n"
    );
    // Real Git index still has staged.txt staged.
    let cached = Command::new("git")
        .current_dir(dir.path())
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&cached.stdout).trim(), "staged.txt");
    // After restore, diff_since is empty.
    assert!(diff_since(root, &cp.tree_oid).unwrap().paths.is_empty());

    prune_ref(root, &cp.ref_name);
}

#[test]
fn non_git_directory_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        capture(dir.path().to_str().unwrap(), "hello").unwrap(),
        None
    );
}

#[test]
fn workspace_records_and_snapshots_turn_checkpoints() {
    use crate::application::{Command, Event, Provider, Reply, Scope, Workspace};
    let mut app = Workspace::default();
    let idx = app.new_session(Scope {
        project: "/projects/demo".into(),
        provider: Provider::Codex,
    });
    app.sessions[idx].remote_id = Some("thread-1".into());
    assert!(app.send(idx, "Update parser".into()));
    let (req_id, _) = app
        .commands()
        .into_iter()
        .find(|(_, cmd)| matches!(cmd, Command::Send(..)))
        .expect("send command");
    let cp = TurnCheckpoint {
        id: "cp-1".into(),
        tree_oid: "0123456789abcdef0123456789abcdef01234567".into(),
        ref_name: "refs/caelum/checkpoints/cp-1".into(),
        prompt_excerpt: "Update parser".into(),
        created_at: 12345,
        turn_id: None,
    };
    app.apply(Event::Completed(req_id, Ok(Reply::Sent(Some(cp.clone())))));
    assert_eq!(app.sessions[idx].checkpoints, vec![cp.clone()]);
    let snap = app.snapshot();
    assert_eq!(snap.sessions[0].checkpoints, vec![cp]);
}
