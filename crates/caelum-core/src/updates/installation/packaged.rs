//! Opt-in real package test, launched by scripts/release/smoke_update.py.
use super::*;
use crate::application::{Attachment, Provider, QueuedPrompt, Scope, Store, Workspace};
use std::{
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires a native display and packaged release; run scripts/release/smoke_update.py"]
fn replacement_relaunch_and_workspace_restoration() {
    let path =
        PathBuf::from(std::env::var_os("CAELUM_UPDATE_TARGET").expect("isolated installed app"));
    let source = std::env::var("CAELUM_UPDATE_SOURCE").expect("loopback package server");
    let name = std::env::var("CAELUM_UPDATE_ASSET").unwrap();
    let root = PathBuf::from(std::env::var_os("CAELUM_UPDATE_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    assert!(path.canonicalize().unwrap().starts_with(&root));
    let kind = if cfg!(target_os = "macos") {
        Kind::Mac
    } else {
        Kind::AppImage
    };
    let destination = Destination {
        identity: Identity::read(&path).unwrap(),
        path,
        kind,
    };
    let project = root.join("project café");
    fs::create_dir(&project).unwrap();
    let project = project.to_str().unwrap();
    let mut app = Workspace::default();
    let index = app.new_session(Scope {
        project: project.into(),
        provider: Provider::Codex,
    });
    app.sessions[index].draft = "Retain this draft after updating ☁️".into();
    app.attach(
        index,
        Attachment::new(
            "example.rs".into(),
            "captured attachment".into(),
            None,
            false,
        ),
    );
    app.sessions[index].queue.push(QueuedPrompt {
        text: "Retain this queued prompt".into(),
        options: Default::default(),
    });
    let store = Store(Store::default_path());
    let snapshot = app.snapshot();
    store.save(&snapshot).unwrap();
    let stage = Stage::open(&destination).unwrap();
    stage.begin(&destination).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        super::super::download::fetch(
            &reqwest::Client::builder().no_proxy().build().unwrap(),
            &format!("{source}/{name}"),
            &format!("{source}/SHA256SUMS"),
            &name,
            &stage.root.join("download"),
            |_| {},
        )
        .await
        .unwrap();
    });
    prepare_payload(&destination, &stage, env!("CARGO_PKG_VERSION"), &|| false).unwrap();
    let mut release = crate::updates::tests::available();
    release.latest_version = env!("CARGO_PKG_VERSION").into();
    let prepared = Arc::new(Prepared::new(release, destination, stage).unwrap());
    let installed = install(prepared.clone()).unwrap();
    assert_eq!(
        Identity::read(&prepared.destination.path).unwrap(),
        prepared.payload_identity
    );
    assert_eq!(
        Identity::read(&prepared.stage.root.join("payload")).unwrap(),
        prepared.destination.identity
    );
    let mut running = Running(installed.relaunch(Some(project)).unwrap());
    assert!(
        !prepared.stage.root.join("receipt.json").exists(),
        "new app must draw and restore its workspace before acknowledging"
    );
    assert!(
        !prepared.stage.root.join("payload").exists(),
        "startup removes the previous version"
    );
    #[cfg(target_os = "macos")]
    assert!(Command::new("/usr/bin/osascript").args(["-l", "JavaScript", "-e", &format!("ObjC.import('AppKit'); $.NSRunningApplication.runningApplicationWithProcessIdentifier({}).terminate;", running.0.id())]).status().unwrap().success());
    #[cfg(target_os = "linux")]
    {
        let window = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--class", "dev.caelum.desktop"])
            .output()
            .unwrap();
        assert!(window.status.success());
        let window = String::from_utf8(window.stdout).unwrap();
        assert!(Command::new("xdotool")
            .args([
                "windowfocus",
                "--sync",
                window.lines().next().unwrap(),
                "key",
                "--clearmodifiers",
                "ctrl+q"
            ])
            .status()
            .unwrap()
            .success());
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = running.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(Instant::now() < deadline, "native quit timed out");
        thread::sleep(Duration::from_millis(100));
    }
    let restored = store.load().unwrap().unwrap();
    assert_eq!(restored.sessions[0].draft, snapshot.sessions[0].draft);
    assert_eq!(
        restored.sessions[0].attachments,
        snapshot.sessions[0].attachments
    );
    assert_eq!(restored.sessions[0].queue, snapshot.sessions[0].queue);
    assert_eq!(restored.selected, Some(0));
}
