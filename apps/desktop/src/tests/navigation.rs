use super::*;

#[test]
fn returning_to_a_project_restores_its_active_worktree_session() {
    use caelum_core::application::{
        Command, Event, Provider, Reply, Scope, SessionScope, Task, TaskReply,
    };
    for provider in Provider::ALL {
        for size in [(800.0, 620.0), (1280.0, 1000.0)] {
            let mut rt = live_runtime_size(size);
            let (mut owner, mut other_session) = (0, 0);
            rt.update(|s, _| {
                let l = s.live.as_mut().unwrap();
                for path in ["/linked", "/other"] {
                    l.app.projects.push(caelum_protocol::ProjectInfo {
                        name: path.into(),
                        path: path.into(),
                    });
                }
                l.app
                    .project_roots
                    .insert("/linked".into(), "/project".into());
                l.app.preferences.session_scope = SessionScope::CurrentProject;
                owner = l.app.new_session(Scope {
                    project: "/linked".into(),
                    provider,
                });
                l.app.sessions[owner].remote_id = Some("active-worktree".into());
                l.app.sessions[owner].conversation.working = true;
                other_session = l.app.new_session(Scope {
                    project: "/other".into(),
                    provider,
                });
                l.app.sessions[other_session].draft = "First other draft".into();
                let next = l.app.new_session(Scope {
                    project: "/other".into(),
                    provider,
                });
                l.app.sessions[next].draft = "Second other draft".into();
                l.app.select(owner);
                l.sync_editors();
            });
            advance(&mut rt, 0.2);
            fill(
                &mut rt,
                &format!("live-draft-{owner}"),
                "Keep this café draft",
            );
            rt.update(|s, _| s.live.as_mut().unwrap().drafts[owner].set_selection(2, 8));
            let count = rt.state.live.as_ref().unwrap().app.sessions.len();
            for _ in 0..2 {
                click(&mut rt, &crate::code_view::source_key("/other", "project"));
                reveal(&mut rt, &format!("live-session-{other_session}"));
                click(&mut rt, &format!("live-session-{other_session}"));

                rt.update(|s, _| {
                    let app = &mut s.live.as_mut().unwrap().app;
                    for (id, command) in app.commands() {
                        if matches!(command, Command::Task(_)) {
                            app.apply(Event::Completed(id, Ok(Reply::Task(TaskReply::Done))));
                        }
                    }
                });
                click(
                    &mut rt,
                    &crate::code_view::source_key("/project", "project"),
                );
                let l = rt.state.live.as_mut().unwrap();
                assert_eq!(l.app.selected, Some(owner));
                assert_eq!(l.app.active_project.as_deref(), Some("/linked"));
                assert_eq!(l.app.sessions.len(), count);
                assert!(l.app.sessions[owner].working());
                assert!(!l.app.sessions[owner].stopping());
                assert_eq!(l.drafts[owner].text, "Keep this café draft");
                assert_eq!((l.drafts[owner].anchor, l.drafts[owner].focus), (2, 8));
                assert_eq!(l.drafts[other_session].text, "First other draft");
                let commands = l.app.commands();
                assert!(commands.iter().any(|(_, c)| matches!(c, Command::Task(Task::Git { project }) if project == "/linked")));
                assert!(!commands.iter().any(|(_, c)| matches!(
                    c,
                    Command::Start(..) | Command::Send(..) | Command::Cancel(_)
                )));
                for (id, command) in commands {
                    if matches!(command, Command::Task(_)) {
                        l.app
                            .apply(Event::Completed(id, Ok(Reply::Task(TaskReply::Done))));
                    }
                }
                assert!(element(&rt, &format!("live-draft-{owner}")).visible[3] > 0.0);
            }
            click(&mut rt, &format!("live-draft-{owner}"));
            key(
                &mut rt,
                Key::Command('z'),
                Mods {
                    cmd: true,
                    ..Default::default()
                },
            );
            assert!(
                rt.state.live.as_ref().unwrap().drafts[owner]
                    .text
                    .is_empty()
            );
            assert_eq!(
                rt.state.live.as_ref().unwrap().drafts[other_session].text,
                "First other draft"
            );
        }
    }
}

#[test]
fn presented_focus_tracks_pointer_tab_accessibility_and_removal() {
    let mut rt = live_runtime();
    let check = |rt: &Runtime<Workspace>| {
        let focused = rt
            .elements()
            .into_iter()
            .find(|e| e.focused && !e.exiting)
            .and_then(|e| e.key);
        assert_eq!(rt.state.live.as_ref().unwrap().focus_key, focused);
    };
    click(&mut rt, "live-draft-0");
    check(&rt);

    for _ in 0..40 {
        key(&mut rt, Key::Tab, Mods::default());
        advance(&mut rt, 0.05);
        check(&rt);
    }
    assert!(rt.accessibility_action("session-actions-1", rune::accesskit::Action::Focus, None));
    advance(&mut rt, 0.4);
    check(&rt);
    assert_eq!(
        rt.state.live.as_ref().unwrap().focus_key.as_deref(),
        Some("session-actions-1")
    );
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(0));
    assert!(element(&rt, "session-actions-1").opacity > 0.9);
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        app.sessions[1].remote_id = Some("deleted-thread".into());
        app.apply(caelum_core::application::Event::OpenCode(
            caelum_protocol::OpenCodeEvent {
                project_path: "/project".into(),
                event_type: "session.deleted".into(),
                data: serde_json::json!({"sessionID":"deleted-thread"}),
                id: None,
                created: None,
                location: None,
            },
        ));
    });
    advance(&mut rt, 0.5);
    check(&rt);
    assert_ne!(
        rt.state.live.as_ref().unwrap().focus_key.as_deref(),
        Some("session-actions-1")
    );
}

#[test]
fn hovering_sessions_keeps_selection_and_switching_keeps_drafts() {
    let mut rt = live_runtime();
    fill(&mut rt, "live-draft-0", "First session's draft");
    click(&mut rt, "live-session-1");
    fill(&mut rt, "live-draft-1", "Second session's draft");
    click(&mut rt, "live-session-0");
    rt.state.live.as_mut().unwrap().app.commands();
    for _ in 0..3 {
        for target in ["live-session-1", "session-actions-1", "live-session-0"] {
            let (x, y) = rt.find(target).unwrap();
            rt.pointer_move(x, y);
            advance(&mut rt, 0.1);
            assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(0));
            assert!(rt.state.live.as_ref().unwrap().session_menu.is_none());
        }
    }
    assert!(rt.state.live.as_mut().unwrap().app.commands().is_empty());
    for i in [1, 0, 1, 0] {
        click(&mut rt, &format!("live-session-{i}"));
        assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(i));
        assert_eq!(
            rt.state.live.as_ref().unwrap().drafts[0].text,
            "First session's draft"
        );
        assert_eq!(
            rt.state.live.as_ref().unwrap().drafts[1].text,
            "Second session's draft"
        );
    }
}

#[test]
#[ignore = "manual session hover/switch CPU profile; run alone with --nocapture"]
fn session_hover_switch_profile() {
    use std::time::Instant;

    for streaming in [false, true] {
        let app = spec();
        let mut state = crate::evaluation::workspace();
        state.live.as_mut().unwrap().evaluation = streaming;
        let mut rt = Runtime::new(state, app.view, (1280.0, 1000.0), Env::new(true, 1.0));
        rt.poll = app.poll;
        rt.on_key = app.on_key;
        advance(&mut rt, 0.6);
        for phase in ["idle", "hover", "switch"] {
            let mut frames = Vec::new();
            let mut focus = Vec::new();
            let commits = rt.stats.commits;
            for i in 0..360 {
                let start = Instant::now();
                if phase != "idle" {
                    let (x, y) = rt.find(&format!("live-session-{}", i / 6 % 6)).unwrap();
                    rt.pointer_move(x, y);
                    if phase == "switch" && i % 6 == 0 {
                        rt.pointer_down(x, y);
                        rt.pointer_up(x, y);
                    }
                }
                rt.frame(1.0 / 60.0);
                let drawn = Instant::now();
                if let Some(focused) = crate::live::presented_focus(&rt) {
                    rt.update(|s, _| crate::live::live_mut(s).focus_key = focused);
                }
                focus.push(drawn.elapsed().as_secs_f64() * 1000.0);
                frames.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            frames.sort_by(f64::total_cmp);
            focus.sort_by(f64::total_cmp);
            println!(
                "streaming={streaming} {phase}: {} nodes, {} commits; frame+focus p50={:.2} p95={:.2} max={:.2} ms; focus p50={:.2} p95={:.2} max={:.2} ms",
                rt.stats.layout_nodes,
                rt.stats.commits - commits,
                frames[180],
                frames[342],
                frames[359],
                focus[180],
                focus[342],
                focus[359]
            );
        }
    }
}
