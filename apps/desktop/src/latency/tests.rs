use super::*;
use caelum_core::application::{Command, Event, Reply, Runtime};
use std::{path::PathBuf, process::Command as Process, time::Duration};

#[test]
fn capture_preserves_navigation_and_joins_the_writer_without_recording_content() {
    let path =
        std::env::temp_dir().join(format!("caelum-latency-test-{}.jsonl", std::process::id()));
    let result = Process::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "latency::tests::capture_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env("CAELUM_LATENCY_TEST", &path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    for secret in [
        "private prompt",
        "private-session",
        "private-event",
        "private-tool",
    ] {
        assert!(!text.contains(secret));
    }
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let has = |kind: &str, label: &str, phase: &str| {
        records
            .iter()
            .any(|r| r["kind"] == kind && r["label"] == label && r["phase"] == phase)
    };
    assert!(has("interaction", "navigate", "abandoned"));
    for (phase, expected) in [("abandoned", 3), ("drawn", 1), ("ready", 1)] {
        assert_eq!(
            records
                .iter()
                .filter(|r| r["kind"] == "interaction"
                    && r["label"] == "navigate"
                    && r["phase"] == phase)
                .count(),
            expected
        );
    }
    assert!(has("interaction", "edit", "drawn"));
    assert!(has("provider", "tool.called", "drawn"));
    for phase in [
        "created",
        "submitted",
        "started",
        "finished",
        "begin",
        "end",
    ] {
        assert!(
            records
                .iter()
                .any(|r| r["kind"] == "request" && r["phase"] == phase),
            "{phase}"
        );
    }
    assert_eq!(records.last().unwrap()["summary"], true);
    assert_eq!(records.last().unwrap()["dropped"], 0);
}

#[test]
#[ignore = "isolated trace subprocess, invoked by the parent regression"]
fn capture_child() {
    let path = PathBuf::from(std::env::var_os("CAELUM_LATENCY_TEST").unwrap());
    let capture = diagnostics::Capture::start(&path).unwrap();
    assert!(diagnostics::Capture::start(&path).is_err());
    let spec = crate::spec();
    let mut state = workspace(false);
    state.live.as_mut().unwrap().latency.replay = false;
    let mut rt = rune::Runtime::new(
        state,
        spec.view,
        (1280.0, 1000.0),
        rune::Env::new(true, 1.0),
    );
    rt.poll = spec.poll;
    rt.set_font_families(spec.families.0, spec.families.1);
    rt.frame(1.0 / 60.0);
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.latency.track(l.latency.begin("navigate"), Some(0), true);
        l.app.select(1);
        l.latency.track(l.latency.begin("navigate"), Some(1), true);
        l.app.select(0);
        l.latency.track(l.latency.begin("navigate"), Some(0), true);
        l.app.select(1);
        l.latency.track(l.latency.begin("navigate"), Some(1), true);
        l.drafts[1].insert("private prompt");
        edit(l);
    });
    rt.frame(1.0 / 60.0);
    drawn(&rt);
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(1));
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[1].text,
        "private prompt"
    );
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[1].scope.provider = caelum_core::application::Provider::OpenCode;
        l.app.sessions[1].remote_id = Some("private-session".into());
        let event = Event::OpenCode(caelum_protocol::OpenCodeEvent {
            project_path: "/evaluation".into(), event_type: "session.tool.called".into(),
            id: Some("private-event".into()), created: Some(100), location: None,
            data: serde_json::json!({"sessionID":"private-session","id":"private-tool","name":"bash","input":{"command":"private prompt"}}),
        });
        diagnostics::event(&event).unwrap().record("received", 100);
        l.apply_events([event]);
    });
    rt.frame(1.0 / 60.0);
    drawn(&rt);
    let mut app = caelum_core::application::Workspace::default();
    app.open_project(".".into());
    let (id, command) = app.commands().pop().unwrap();
    assert!(matches!(command, Command::Open(_)));
    let (wake, receiver) = std::sync::mpsc::channel();
    let mut runtime = Runtime::new(move || {
        let _ = wake.send(());
    });
    runtime.submit(id, command).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let mut completed = false;
        for event in runtime.drain() {
            completed |= matches!(&event, Event::Completed(request, Ok(Reply::Project(_))) if *request == id);
            app.apply(event);
        }
        if completed {
            break;
        }
        receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
            .unwrap();
    }
    drop(runtime);
    drop(capture);
}
