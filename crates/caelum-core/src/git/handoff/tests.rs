use super::*;
use std::{fs, path::Path, process::Command};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn new_vm_refs_publish_only_the_remote_main_commit_and_never_replace_an_existing_ref() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    git(root, &["init", "--bare", "-b", "main", "origin.git"]);
    git(root, &["init", "-b", "main", "local"]);
    let local = root.join("local");
    let remote = root.join("origin.git");
    git(&local, &["commit", "--allow-empty", "-m", "published"]);
    git(&local, &["push", "../origin.git", "main"]);
    git(
        &local,
        &["remote", "add", "origin", "https://github.com/example/repo"],
    );
    let pinned = git(&local, &["rev-parse", "HEAD"]);
    git(&local, &["commit", "--allow-empty", "-m", "not published"]);
    let script = root.join("git-wrapper");
    crate::test_support::write_executable(&script, "#!/bin/sh\nif test \"$1\" = ls-remote; then exec git -c url.\"$PWD/../origin.git\".insteadOf=https://github.com/example/repo \"$@\"; fi\nexec git \"$@\"\n");
    let client = GitClient::with_executable(script);
    let project = local.to_str().unwrap();
    for kind in [
        BranchPublication::OnMain,
        BranchPublication::Detached,
        BranchPublication::Named("feat/local".into()),
    ] {
        match &kind {
            BranchPublication::OnMain => (),
            BranchPublication::Detached => {
                git(&local, &["switch", "--detach"]);
            }
            BranchPublication::Named(b) => {
                git(&local, &["switch", "-c", b]);
            }
        }
        let (base, publication) = client.vm_base(project).await.unwrap();
        assert_eq!(base.tip, pinned);
        assert_eq!(publication.as_ref(), Some(&kind));
        let token = uuid::Uuid::new_v4().to_string();
        let branch = kind.destination(&token);
        let source = Source {
            project: project.into(),
            repository: "example/repo".into(),
            account: "example".into(),
            base: branch.clone(),
            publish: publication,
            published_tip: Some(pinned.clone()),
            environment: None,
        };
        assert!(client.origin_tip(project, &branch).await.unwrap().is_none());
        let denied = client
            .ensure_vm_branch(&source, &token, |_, _| Err("denied".into()))
            .await;
        assert_eq!(denied.unwrap_err(), "denied");
        client
            .ensure_vm_branch(&source, &token, |s, tip| {
                git(
                    &remote,
                    &["update-ref", &format!("refs/heads/{}", s.base), tip, ""],
                );
                Err("response lost".into())
            })
            .await
            .unwrap();
        let no_write = |_: &Source, _: &str| -> Result<(), String> {
            panic!("Existing ref must not be updated")
        };
        client
            .ensure_vm_branch(&source, &token, no_write)
            .await
            .unwrap();
        assert_eq!(git(&remote, &["rev-parse", &branch]), pinned);
        assert_eq!(
            client
                .destination_base(project, Some(&(kind.clone(), branch.clone())))
                .await
                .unwrap()
                .branch,
            branch
        );
        let conflict = git(
            &remote,
            &[
                "commit-tree",
                &format!("{pinned}^{{tree}}"),
                "-p",
                &pinned,
                "-m",
                "Someone else's commit",
            ],
        );
        git(
            &remote,
            &[
                "update-ref",
                &format!("refs/heads/{branch}"),
                &conflict,
                &pinned,
            ],
        );
        assert!(client
            .ensure_vm_branch(&source, &token, no_write)
            .await
            .unwrap_err()
            .contains("different commit"));
        assert_eq!(git(&remote, &["rev-parse", &branch]), conflict);
    }
    git(&local, &["switch", "-c", "another"]);
    git(&remote, &["update-ref", "-d", "refs/heads/main"]);
    assert!(client
        .vm_base(project)
        .await
        .unwrap_err()
        .contains("No published origin/main"));
    fs::rename(&remote, root.join("offline")).unwrap();
    assert!(client
        .vm_base(project)
        .await
        .unwrap_err()
        .contains("Could not read"));
}
