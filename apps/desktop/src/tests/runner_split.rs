use super::*;
use caelum_core::{
    application::{Command, Event},
    runner::RunnerEvent,
};

#[test]
fn checkout_command_runner_runs_stops_reruns_and_attaches_output() {
    let mut rt = live_runtime();
    rt.update(|s, _| crate::runner::open(s));
    advance(&mut rt, 0.25);
    assert!(rt.find("runner-panel").is_some());

    fill(&mut rt, "runner-command", "cargo test");
    click(&mut rt, "runner-run");
    let cmds = rt.state.live.as_mut().unwrap().app.commands();
    let run_id = cmds
        .iter()
        .find_map(|(_, c)| match c {
            Command::RunCommand {
                project,
                run_id,
                command,
            } if project == "/project" && command == "cargo test" => Some(*run_id),
            _ => None,
        })
        .expect("expected RunCommand");

    // Stop button dispatches StopCommand while running.
    click(&mut rt, "runner-stop");
    assert!(rt
        .state
        .live
        .as_mut()
        .unwrap()
        .app
        .commands()
        .iter()
        .any(|(_, c)| matches!(c, Command::StopCommand { project, run_id: id } if project == "/project" && *id == run_id)));

    // Apply finished output and attach to the active session prompt.
    rt.update(|s, _| {
        s.live
            .as_mut()
            .unwrap()
            .apply(Event::Runner(RunnerEvent::Finished {
                project: "/project".into(),
                run_id,
                code: Some(1),
                duration_ms: 1200,
                output: "test failed: assertion error".into(),
                truncated: false,
                cancelled: false,
            }));
    });
    advance(&mut rt, 0.1);
    click(&mut rt, "runner-attach");
    let l = rt.state.live.as_ref().unwrap();
    assert!(l.panels.overlay().is_none());
    assert_eq!(l.app.sessions[0].attachments.len(), 1);
    assert!(
        l.app.sessions[0].attachments[0]
            .content
            .contains("test failed: assertion error")
    );
}

#[test]
fn split_pane_routes_focus_preserves_drafts_and_collapses_on_compact_width() {
    let mut rt = live_runtime_size((1280.0, 900.0));
    fill(&mut rt, "live-draft-0", "Primary draft");

    rt.update(|s, _| crate::split::open_session_on(s, 1, crate::split::SplitSide::Right));
    advance(&mut rt, 0.2);
    assert!(rt.find("split-secondary-pane").is_some());
    assert!(rt.find("split-composer-target").is_some());
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Secondary
    );

    fill(&mut rt, "live-draft-1", "Secondary draft");
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[0].text,
        "Primary draft"
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[1].text,
        "Secondary draft"
    );

    // Clicking the target pills in the shared composer switches active pane and draft.
    click(&mut rt, "split-target-left");
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Primary
    );
    assert!(rt.find("live-draft-0").is_some());
    click(&mut rt, "split-target-right");
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Secondary
    );
    assert!(rt.find("live-draft-1").is_some());

    // ⌥⌘← switches focus to primary; ⌥⌘→ switches back to secondary.
    key(
        &mut rt,
        Key::Left,
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Primary
    );
    key(
        &mut rt,
        Key::Right,
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Secondary
    );

    // Escape stops a working secondary session without stopping primary.
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[0].remote_id = Some("s0".into());
        l.app.sessions[0].conversation.set_working(true);
        l.app.sessions[1].remote_id = Some("s1".into());
        l.app.sessions[1].conversation.set_working(true);
    });
    let _ = rt.state.live.as_mut().unwrap().app.commands();
    key(&mut rt, Key::Escape, Mods::default());
    assert!(
        rt.state
            .live
            .as_mut()
            .unwrap()
            .app
            .commands()
            .iter()
            .any(|(_, c)| matches!(c, Command::Cancel(k) if k.id == "s1"))
    );

    // Closing the split pane returns cleanly to primary.
    click(&mut rt, "split-close");
    assert!(rt.find("split-secondary-pane").is_none());

    // Split left places secondary on the left and flips spatial focus routing.
    rt.update(|s, _| crate::split::open_session_on(s, 1, crate::split::SplitSide::Left));
    advance(&mut rt, 0.2);
    assert!(rt.find("split-divider").is_some());
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.side,
        crate::split::SplitSide::Left
    );
    key(
        &mut rt,
        Key::Right,
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Primary
    );
    key(
        &mut rt,
        Key::Left,
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Secondary
    );
    click(&mut rt, "split-close");

    // Compact window width (< 1100pt) collapses split pane automatically.
    let mut compact = live_runtime_size((960.0, 720.0));
    compact.update(|s, _| crate::split::open_session_on(s, 1, crate::split::SplitSide::Right));
    advance(&mut compact, 0.2);
    assert!(compact.find("split-secondary-pane").is_none());
}
