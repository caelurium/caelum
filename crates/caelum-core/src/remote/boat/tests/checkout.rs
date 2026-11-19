use super::{environment, fixture, sandbox};
use crate::remote::{harness::Harness, GitAction, Machines, Outcome, Plan, Size, Source, Target};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, Request, ResponseTemplate,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.com")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.com")
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout)
        .trim_end()
        .to_owned()
}

#[tokio::test]
async fn first_checkout_configures_only_its_new_environment_before_creating_the_vm() {
    let (server, boat) = fixture().await;
    let plan = Plan {
        token: uuid::Uuid::new_v4().to_string(),
        branch: "feat/new".into(),
        size: Size::Small,
        minutes: 30,
    };
    let name = format!("caelum-{}", plan.token);
    let environment = |id: &str, name: &str, is_default: bool| {
        json!({"id":id,"name":name,"isDefault":is_default,
        "safeForThirdParties":false,"passGithub":true,"passSecrets":false,"passSandboxCredentials":false,
        "passAgentsCredentials":true,"envContents":"","secretFiles":[],"versions":[],"selectedRepositories":[]})
    };
    Mock::given(method("GET"))
        .and(path("/environments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "environments":[environment("shared","base",true)]
        })))
        .mount(&server)
        .await;
    Mock::given(path("/repos")).and(wiremock::matchers::query_param("q","example/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"repos","environmentId":"shared",
            "installations":[{"repositories":[{"fullName":"other/project","databaseId":"wrong"},{"fullName":"example/project","databaseId":"repo-1"}]}],"selectedRepositories":[]}))).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/environments"))
        .and(body_json(json!({"name":name})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"environments":[environment("dedicated",&name,false)]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PUT")).and(path("/environments/dedicated")).and(body_json(json!({
        "safeForThirdParties":false,"passGithub":true,"passSecrets":false,"passSandboxCredentials":false,
        "passAgentsCredentials":true,"repositories":[{"repositoryId":"repo-1","baseBranch":"main"}]
    }))).respond_with(ResponseTemplate::new(200).set_body_json(json!({"success":true}))).expect(1).mount(&server).await;
    Mock::given(method("POST")).and(path("/sandboxes")).and(header("idempotency-key",plan.token.as_str()))
        .and(wiremock::matchers::body_partial_json(json!({"environment":name})))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({"ok":true,"type":"create","status":"provisioning","sandbox":sandbox("provisioning")}))).expect(1).mount(&server).await;
    let source = Source {
        project: "/local/project".into(),
        account: boat.account.clone(),
        repository: "example/project".into(),
        base: "main".into(),
        environment: None,
    };
    assert_eq!(boat.create(&source, &plan).await.unwrap().key.id, "vm-1");
}

#[tokio::test]
async fn opens_a_new_branch_runs_opencode_and_publishes_only_the_reviewed_vm_tree() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let laptop = root.join("laptop/project");
    let vm = root.join("vm/project");
    fs::create_dir_all(&laptop).unwrap();
    fs::create_dir_all(vm.parent().unwrap()).unwrap();
    git(root, &["init", "--bare", "-b", "main", "published.git"]);
    git(&laptop, &["init", "-b", "main"]);
    fs::write(laptop.join("README"), "Published\n").unwrap();
    git(&laptop, &["add", "README"]);
    git(&laptop, &["commit", "-m", "initial"]);
    let published = root.join("published.git");
    git(&laptop, &["push", published.to_str().unwrap(), "main"]);
    git(
        root,
        &["clone", published.to_str().unwrap(), vm.to_str().unwrap()],
    );
    git(
        &vm,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/example/project.git",
        ],
    );
    git(&laptop, &["switch", "-c", "laptop-only"]);
    fs::write(laptop.join("README"), "Unpublished laptop work\n").unwrap();
    let laptop_before = git(&laptop, &["diff"]);
    let (server, boat) = fixture().await;
    environment(&server).await;
    let plan = Plan {
        token: uuid::Uuid::new_v4().to_string(),
        branch: "feat/remote".into(),
        size: Size::Small,
        minutes: 30,
    };
    Mock::given(method("POST"))
        .and(path("/sandboxes"))
        .and(header("idempotency-key", plan.token.as_str()))
        .respond_with(ResponseTemplate::new(202).set_body_json(
            json!({"ok":true,"type":"create","status":"provisioning","sandbox":sandbox("cloning")}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/sandboxes/vm-1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"ok":true,"type":"info","sandbox":sandbox("idle")})),
        )
        .mount(&server)
        .await;
    let wd = vm.clone();
    let upstream = published.clone();
    // The fixture substitutes only the VM mount and the network transport. The actual Git
    // branch, diff, index, commit and push implementations run against two separate clones.
    Mock::given(method("POST")).and(path("/sandboxes/vm-1/commands")).and(header("authorization","Bearer fixture-key"))
        .respond_with(move |req:&Request| {
            let body:Value=req.body_json().unwrap();
            let script=body["command"].as_str().unwrap().strip_prefix("cd '/home/user/project' && ").unwrap();
            let transport=r#"git() { case "$1" in fetch|push) command git -c "url.$UPSTREAM.insteadOf=https://github.com/example/project.git" "$@";; *) command git "$@";; esac; }"#;
            let result=Command::new("/bin/sh").arg("-c").arg(format!("{transport}\n{script}")).current_dir(&wd)
                .env("UPSTREAM", &upstream).env("GIT_CONFIG_GLOBAL","/dev/null").env("GIT_CONFIG_NOSYSTEM","1")
                .env("GIT_AUTHOR_NAME","Fixture").env("GIT_AUTHOR_EMAIL","fixture@example.com")
                .env("GIT_COMMITTER_NAME","Fixture").env("GIT_COMMITTER_EMAIL","fixture@example.com").output().unwrap();
            ResponseTemplate::new(200).set_body_json(json!({"ok":true,"type":"exec","success":result.status.success(),"exitCode":result.status.code(),
                "timedOut":false,"stdout":String::from_utf8_lossy(&result.stdout),"stderr":String::from_utf8_lossy(&result.stderr)}))
        }).mount(&server).await;
    let agent_dir = vm.clone();
    Mock::given(method("POST")).and(path("/sandboxes/vm-1/prompt")).and(body_json(json!({"provider":"opencode","new":true,"prompt":"Update the README"})))
        .respond_with(move |_:&Request| {
            assert_eq!(git(&agent_dir,&["branch","--show-current"]),"feat/remote");
            fs::write(agent_dir.join("README"),"Reviewed remote edit\n").unwrap();
            fs::write(agent_dir.join("notes.txt"),"New remote file\n").unwrap();
            ResponseTemplate::new(202).set_body_json(json!({"ok":true,"type":"queued","id":"vm-1","promptId":"p","conversationId":"c",
                "promptRun":{"id":"r","promptId":"p","sandboxId":"vm-1","status":"queued","done":false},"status":"queued","provider":"opencode"}))
        }).expect(1).mount(&server).await;
    let source = Source {
        project: laptop.to_string_lossy().into(),
        account: "user-1".into(),
        repository: "example/project".into(),
        base: "main".into(),
        environment: Some("project-env".into()),
    };
    let machine = boat.create(&source, &plan).await.unwrap();
    let target = Target {
        machine: machine.key,
        repository: source.repository,
    };
    let Outcome::Prepared(_, branch) = boat
        .prepare(&target, "main", Some("feat/remote"))
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(branch, "feat/remote");
    assert!(matches!(
        boat.send(&target, None, "Update the README").await.unwrap(),
        Outcome::Accepted(_)
    ));
    let review = boat.git(&target, &GitAction::Read).await.unwrap();
    assert!(
        review.patch.contains("+Reviewed remote edit") && review.patch.contains("+New remote file")
    );
    assert!(git(&vm, &["diff", "--cached"]).is_empty());
    fs::write(vm.join("README"), "Later edit\n").unwrap();
    let committed = boat
        .git(
            &target,
            &GitAction::Commit(review.revision.clone(), "feat: developer's edit".into()),
        )
        .await
        .unwrap();
    assert!(committed.patch.contains("+Later edit"));
    assert!(boat
        .git(&target, &GitAction::Commit(review.revision, "stale".into()))
        .await
        .is_err());
    boat.git(&target, &GitAction::Push(committed.revision))
        .await
        .unwrap();
    assert_eq!(
        git(&published, &["show", "feat/remote:README"]),
        "Reviewed remote edit"
    );
    assert_eq!(
        git(&published, &["show", "feat/remote:notes.txt"]),
        "New remote file"
    );
    assert_eq!(git(&laptop, &["branch", "--show-current"]), "laptop-only");
    assert_eq!(git(&laptop, &["diff"]), laptop_before);
    assert!(!laptop.join("notes.txt").exists());
}
