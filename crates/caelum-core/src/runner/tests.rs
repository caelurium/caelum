use super::*;
use std::sync::mpsc;

#[test]
fn ring_buffer_truncates_from_front_preserving_utf8() {
    let mut ring = RingBuffer::default();
    let prefix = "é".repeat(OUTPUT_LIMIT / 2 + 10);
    ring.push(prefix.as_bytes());
    ring.push(b"TAIL");
    assert!(ring.truncated());
    let text = ring.text();
    assert!(text.ends_with("TAIL"));
    assert!(text.len() <= OUTPUT_LIMIT);
}

#[test]
fn workspace_runner_tracks_history_and_rejects_stale_run_events() {
    let mut runner = WorkspaceRunner::default();
    assert!(runner.start("/repo", "   ".into()).is_none());
    let (id1, _) = runner.start("/repo", "cargo test".into()).unwrap();
    let (id2, _) = runner.start("/repo", "make check".into()).unwrap();
    assert_eq!(
        runner.get("/repo").unwrap().history,
        vec!["make check", "cargo test"]
    );

    // Stale event from run 1 must be ignored.
    assert!(!runner.apply(RunnerEvent::Output {
        project: "/repo".into(),
        run_id: id1,
        output: "stale".into(),
        truncated: false,
    }));
    assert!(!runner.apply(RunnerEvent::Finished {
        project: "/repo".into(),
        run_id: id1,
        code: Some(0),
        duration_ms: 10,
        output: "stale".into(),
        truncated: false,
        cancelled: false,
    }));
    assert!(runner.get("/repo").unwrap().state.running());

    // Current run 2 updates and exits cleanly.
    assert!(runner.apply(RunnerEvent::Output {
        project: "/repo".into(),
        run_id: id2,
        output: "ok\n".into(),
        truncated: false,
    }));
    assert!(runner.apply(RunnerEvent::Finished {
        project: "/repo".into(),
        run_id: id2,
        code: Some(0),
        duration_ms: 42,
        output: "ok\n".into(),
        truncated: false,
        cancelled: false,
    }));
    assert!(matches!(
        runner.get("/repo").unwrap().state,
        RunState::Exited {
            code: Some(0),
            duration_ms: 42,
            ..
        }
    ));
}

#[cfg(unix)]
#[test]
fn backend_runs_streams_and_cancels_commands_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().to_str().unwrap().to_owned();
    let processes = Arc::new(ProcessRegistry::default());
    let backend = Backend::new(processes.clone());
    let (tx, rx) = mpsc::sync_channel(64);
    backend.observe(tx, Arc::new(|| {}));

    backend
        .start(&project, 1, "printf 'hello\\n'; printf 'err\\n' >&2")
        .unwrap();
    let finished = loop {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Runner(RunnerEvent::Finished {
                run_id: 1,
                code,
                output,
                cancelled,
                ..
            }) => {
                break (code, output, cancelled);
            }
            _ => continue,
        }
    };
    assert_eq!(finished.0, Some(0));
    assert!(!finished.2);
    assert!(finished.1.contains("hello"));
    assert!(finished.1.contains("err"));

    // Cancellation terminates a long-running process and reports cancelled = true.
    backend.start(&project, 2, "sleep 30 & wait").unwrap();
    thread::sleep(Duration::from_millis(80));
    backend.stop(&project, 2);
    let cancelled = loop {
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Runner(RunnerEvent::Finished {
                run_id: 2,
                cancelled,
                ..
            }) => break cancelled,
            _ => continue,
        }
    };
    assert!(cancelled);
}
