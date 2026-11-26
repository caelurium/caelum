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
fn split_pane_supports_vertical_horizontal_and_offscreen_deck_rotation() {
    let mut rt = live_runtime_size((1280.0, 900.0));
    // Add a 3rd session so we can test off-screen deck rotation.
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let scope = l.app.sessions[0].scope.clone();
        l.app.project_draft(scope);
        l.sync_editors();
        l.app.select(0);
    });
    fill(&mut rt, "live-draft-0", "Primary draft");

    rt.update(|s, _| {
        crate::split::open_session(s, 1, crate::split::SplitOrientation::Vertical);
    });
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

    // Clicking the target pills switches active pane and draft.
    click(&mut rt, "split-target-primary");
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Primary
    );
    assert!(rt.find("live-draft-0").is_some());
    click(&mut rt, "split-target-secondary");
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.active,
        crate::split::ActivePane::Secondary
    );
    assert!(rt.find("live-draft-1").is_some());

    // Opening a 3rd session slides session 0 off-screen into the deck (visible: 1 | 2, offscreen: 0).
    rt.update(|s, _| {
        crate::split::open_session(s, 2, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut rt, 0.2);
    {
        let l = rt.state.live.as_ref().unwrap();
        assert_eq!(l.app.selected, Some(1));
        assert_eq!(
            l.split.visible_secondary(),
            Some(&crate::split::SecondaryPane::Session(2))
        );
        assert_eq!(l.split.offscreen_count(), 1);
    }
    assert!(rt.find("split-target-offscreen-0").is_some());

    // ⌥⌘] rotates session 0 back into view (visible: 2 | 0, offscreen: 1).
    key(
        &mut rt,
        Key::Command(']'),
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    {
        let l = rt.state.live.as_ref().unwrap();
        assert_eq!(l.app.selected, Some(2));
        assert_eq!(
            l.split.visible_secondary(),
            Some(&crate::split::SecondaryPane::Session(0))
        );
    }

    // ⌥⌘[ rotates back (visible: 1 | 2).
    key(
        &mut rt,
        Key::Command('['),
        Mods {
            cmd: true,
            alt: true,
            ..Default::default()
        },
    );
    {
        let l = rt.state.live.as_ref().unwrap();
        assert_eq!(l.app.selected, Some(1));
        assert_eq!(
            l.split.visible_secondary(),
            Some(&crate::split::SecondaryPane::Session(2))
        );
    }

    // Clicking the off-screen pill brings session 0 back into view immediately.
    click(&mut rt, "split-target-offscreen-0");
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.visible_secondary(),
        Some(&crate::split::SecondaryPane::Session(0))
    );

    // Toggle to horizontal split (top/bottom) and navigate with ⌥⌘↑ / ⌥⌘↓.
    click(&mut rt, "split-toggle-orientation");
    advance(&mut rt, 0.2);
    assert_eq!(
        rt.state.live.as_ref().unwrap().split.orientation,
        crate::split::SplitOrientation::Horizontal
    );
    assert!(rt.find("split-top-pane").is_some());
    assert!(rt.find("split-bottom-pane").is_some());
    assert!(rt.find("split-bottom-header").is_some());
    key(
        &mut rt,
        Key::Up,
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
        Key::Down,
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

    // Escape stops the active working secondary session without stopping primary.
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[0].remote_id = Some("s0".into());
        l.app.sessions[0].conversation.set_working(true);
        l.app.sessions[2].remote_id = Some("s2".into());
        l.app.sessions[2].conversation.set_working(true);
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
            .any(|(_, c)| matches!(c, Command::Cancel(k) if k.id == "s0"))
    );

    // Opening Overview while Secondary is active takes over Secondary and keeps the composer for Primary.
    rt.update(crate::overview::open);
    advance(&mut rt, 0.2);
    {
        let l = rt.state.live.as_ref().unwrap();
        assert_eq!(
            l.split.visible_secondary(),
            Some(&crate::split::SecondaryPane::Overview)
        );
        assert_eq!(l.panels.screen, crate::panels::Screen::Conversation);
        assert_eq!(crate::split::composer_target(l), l.app.selected);
    }
    assert!(rt.find("message-composer").is_some());

    // Compact window width (< 1100pt) collapses vertical split, while horizontal stays visible.
    let mut compact = live_runtime_size((960.0, 720.0));
    compact.update(|s, _| {
        crate::split::open_session(s, 1, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut compact, 0.2);
    assert!(compact.find("split-secondary-pane").is_none());
    compact.update(|s, _| crate::split::toggle_orientation(s));
    advance(&mut compact, 0.2);
    assert!(compact.find("split-bottom-pane").is_some());
}

#[test]
fn composer_target_bar_keeps_long_titles_vertically_centered_and_within_the_pill() {
    let mut rt = live_runtime_size((1280.0, 900.0));
    let first_line =
        "find an implicit state machine in this repository and replace it with an explicit enum";
    let multiline_title = format!("{first_line}\n\nMore details on a later line");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[0].title = multiline_title.clone();
        l.app.sessions[1].title = "Feature worktree".into();
        crate::split::open_session(s, 1, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut rt, 0.2);
    click(&mut rt, "split-target-primary");
    let pill = element(&rt, "split-target-primary").rect;
    let label = rt
        .elements()
        .iter()
        .find(|e| {
            e.text.as_deref().is_some_and(|t| t.starts_with(first_line))
                && e.rect[0] > 250.0
                && e.rect[1] > 600.0
        })
        .expect("missing primary target title")
        .rect;
    assert!(
        label[1] >= pill[1] - 0.5 && label[1] + label[3] <= pill[1] + pill[3] + 0.5,
        "target pill title clipped vertically: pill={pill:?} label={label:?}"
    );
    assert!(
        label[0] >= pill[0] && label[0] + label[2] <= pill[0] + pill[2] + 0.5,
        "target pill title overflowed horizontally: pill={pill:?} label={label:?}"
    );
}

#[test]
fn remote_and_home_screens_do_not_render_a_second_shared_composer_in_split() {
    let mut rt = live_runtime_size((1280.0, 900.0));
    rt.update(|s, _| {
        crate::split::open_session(s, 1, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut rt, 0.2);
    assert!(rt.find("message-composer").is_some());

    for screen in [crate::panels::Screen::Remote, crate::panels::Screen::Home] {
        rt.update(|s, _| {
            s.live.as_mut().unwrap().panels.screen = screen;
        });
        advance(&mut rt, 0.2);
        assert_eq!(
            crate::split::composer_target(rt.state.live.as_ref().unwrap()),
            None
        );
        assert!(
            rt.find("message-composer").is_none(),
            "shared composer should not stack below {screen:?} in a split"
        );
    }
}

#[test]
fn splitting_projects_keeps_primary_syncs_active_project_and_creates_draft_when_empty() {
    use caelum_protocol::ProjectInfo;

    let mut rt = live_runtime_size((1280.0, 900.0));
    let project_a = rt.state.live.as_ref().unwrap().app.sessions[0]
        .scope
        .project
        .clone();
    let project_b = "/workspace/beta".to_owned();
    let project_c = "/workspace/gamma".to_owned();
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.projects.push(ProjectInfo {
            name: "beta".into(),
            path: project_b.clone(),
        });
        l.app.projects.push(ProjectInfo {
            name: "gamma".into(),
            path: project_c.clone(),
        });
        let b_idx = l.app.new_session(caelum_core::application::Scope {
            project: project_b.clone(),
            provider: caelum_core::application::Provider::Codex,
        });
        l.app.sessions[b_idx].title = "Beta work".into();
        l.app.select(0);
        l.sync_editors();
    });
    advance(&mut rt, 0.2);
    assert_eq!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .active_project
            .as_deref(),
        Some(project_a.as_str())
    );

    // Click the sidebar split button on project_b.
    let split_b = crate::code_view::source_key(&project_b, "split-project");
    reveal(&mut rt, &split_b);
    click(&mut rt, &split_b);
    advance(&mut rt, 0.2);

    let l = rt.state.live.as_ref().unwrap();
    assert_eq!(l.app.selected, Some(0));
    let Some(crate::split::SecondaryPane::Session(sec_b)) = l.split.visible_secondary().cloned()
    else {
        panic!("expected beta session in secondary pane");
    };
    assert_eq!(l.app.sessions[sec_b].scope.project, project_b);
    assert_eq!(l.split.active, crate::split::ActivePane::Secondary);
    assert_eq!(l.app.active_project.as_deref(), Some(project_b.as_str()));

    // Focusing back to primary syncs active_project back to project_a.
    click(&mut rt, "split-target-primary");
    advance(&mut rt, 0.1);
    let l = rt.state.live.as_ref().unwrap();
    assert_eq!(l.split.active, crate::split::ActivePane::Primary);
    assert_eq!(l.app.active_project.as_deref(), Some(project_a.as_str()));

    // Splitting a project with no sessions creates a draft for it in the split pane.
    rt.update(|s, _| {
        crate::split::open_project(s, &project_c, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut rt, 0.2);
    let l = rt.state.live.as_ref().unwrap();
    let Some(crate::split::SecondaryPane::Session(sec_c)) = l.split.visible_secondary().cloned()
    else {
        panic!("expected gamma draft session in secondary pane");
    };
    assert_eq!(l.app.sessions[sec_c].scope.project, project_c);
    assert_eq!(l.app.active_project.as_deref(), Some(project_c.as_str()));
}
