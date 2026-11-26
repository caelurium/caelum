use super::*;
use crate::{
    live::{live, live_mut},
    panels::Screen,
};
use caelum_core::application::{Attachment, Command, Event, Provider, Reply};

#[test]
fn home_keeps_native_selection_undo_and_both_drafts_across_navigation() {
    let mut rt = live_runtime();
    fill(&mut rt, "live-draft-0", "Session draft");
    click(&mut rt, "nav-home");
    fill(&mut rt, "home-draft", "A café idea");
    rt.update(|s, _| live_mut(s).home.editor.set_selection(2, 7));
    let selection = (
        live(&rt.state).home.editor.anchor,
        live(&rt.state).home.editor.focus,
    );
    rt.update(|s, _| live_mut(s).app.home_select_project("/project".into()));
    advance(&mut rt, 0.1);
    click(&mut rt, "live-session-0");
    assert_eq!(live(&rt.state).drafts[0].text, "Session draft");
    click(&mut rt, "nav-home");
    assert_eq!(live(&rt.state).home.editor.text, "A café idea");
    assert_eq!(
        (
            live(&rt.state).home.editor.anchor,
            live(&rt.state).home.editor.focus
        ),
        selection
    );
    assert!(element(&rt, "home-draft").focused);
    key(
        &mut rt,
        Key::Command('z'),
        Mods {
            cmd: true,
            ..Default::default()
        },
    );
    assert_eq!(live(&rt.state).home.editor.text, "");
    assert_eq!(live(&rt.state).drafts[0].text, "Session draft");
}

#[test]
fn home_send_moves_context_into_a_fresh_session_and_focuses_its_editor() {
    let mut rt = design_runtime("home", (1280.0, 1000.0));
    let previous = live(&rt.state).drafts[0].text.clone();
    rt.update(|s, _| {
        let l = live_mut(s);
        l.app.home_set_provider(Provider::OpenCode);
        l.app.home.draft.options.model = Some("home-model".into());
        l.app.home.draft.attachments.push(Attachment::new(
            "context.rs".into(),
            "Captured file".into(),
            None,
            false,
        ));
    });
    fill(&mut rt, "home-draft", "Start a fresh task");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    let l = live(&rt.state);
    let index = l.app.selected.unwrap();
    assert_eq!(index, 3);
    assert_eq!(l.panels.screen, Screen::Conversation);
    assert_eq!(l.app.sessions[index].scope.provider, Provider::OpenCode);
    assert_eq!(
        l.app.sessions[index].options.model.as_deref(),
        Some("home-model")
    );
    assert!(l.app.sessions[index].working());
    assert_eq!(l.drafts[0].text, previous);
    assert!(l.home.editor.text.is_empty());
    assert!(l.app.home.draft.attachments.is_empty());
    assert!(element(&rt, &format!("live-draft-{index}")).focused);
    click(&mut rt, "nav-home");
    assert!(live(&rt.state).home.editor.text.is_empty());
}

#[test]
fn cancelling_the_inline_project_chooser_preserves_input_and_creates_no_session() {
    let mut rt = design_runtime("home-empty", (800.0, 620.0));
    let count = live(&rt.state).app.sessions.len();
    fill(&mut rt, "home-draft", "An idea before choosing a project");
    click(&mut rt, "home-project");
    click(&mut rt, "home-open-project");
    assert!(rt.file_dialog().is_some());
    rt.resolve_file_dialog(None).unwrap();
    advance(&mut rt, 0.2);
    assert_eq!(
        live(&rt.state).home.editor.text,
        "An idea before choosing a project"
    );
    assert_eq!(live(&rt.state).app.sessions.len(), count);
    assert_eq!(live(&rt.state).panels.screen, Screen::Home);
    click(&mut rt, "home-open-project");
    rt.resolve_file_dialog(Some("/chosen".into())).unwrap();
    advance(&mut rt, 0.2);
    rt.update(|s, _| {
        let l = live_mut(s);
        let id = l
            .app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Open(_)))
            .unwrap()
            .0;
        l.apply(Event::Completed(
            id,
            Ok(Reply::Project(caelum_protocol::ProjectInfo {
                name: "Chosen".into(),
                path: "/chosen".into(),
            })),
        ));
    });
    advance(&mut rt, 0.2);
    assert_eq!(
        live(&rt.state).app.home.draft.project.as_deref(),
        Some("/chosen")
    );
    assert_eq!(live(&rt.state).app.sessions.len(), count);
    assert_eq!(
        live(&rt.state).home.editor.text,
        "An idea before choosing a project"
    );
}

#[test]
fn general_chat_starts_from_home_and_attaches_to_a_project_from_heading() {
    let mut rt = design_runtime("home", (1280.0, 900.0));
    // Switch Home to General (unbound) and start a chat.
    rt.update(|s, _| live_mut(s).app.home_select_general());
    advance(&mut rt, 0.2);
    fill(&mut rt, "home-draft", "Quick general question");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.2);

    let index = live(&rt.state).app.selected.unwrap();
    assert!(live(&rt.state).app.sessions[index].unbound);
    assert!(rt.find("attach-project").is_some());

    // Complete the initial Start command so the session is idle and can be attached to an open project.
    let target = live(&rt.state).app.projects[0].path.clone();
    rt.update(|s, _| {
        let l = live_mut(s);
        let start_id = l
            .app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Start(_, _)))
            .expect("expected Start command")
            .0;
        let scope = l.app.sessions[index].scope.clone();
        l.apply(Event::Completed(
            start_id,
            Ok(Reply::Session(caelum_core::application::Summary {
                key: caelum_core::application::SessionKey {
                    scope,
                    id: "general-1".into(),
                },
                title: "Quick general question".into(),
                updated_at: None,
                working: false,
                options: Default::default(),
            })),
        ));
        if let Some((send_id, _)) = l
            .app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Send(_, _, _)))
        {
            l.apply(Event::Completed(send_id, Ok(Reply::Done)));
        }
        l.app.sessions[index].conversation.set_working(false);
    });
    advance(&mut rt, 0.2);

    click(&mut rt, "attach-project");
    assert!(rt.find("attach-project-menu").is_some());
    click(&mut rt, &format!("attach-project-{target}"));
    advance(&mut rt, 0.2);

    let l = live(&rt.state);
    assert!(!l.app.sessions[index].unbound);
    assert_eq!(l.app.sessions[index].scope.project, target);
    assert!(rt.find("attach-project").is_none());

    // Returning to Home with General selected shows only General chats in the sidebar;
    // selecting the attached project shows that project's sessions in the sidebar.
    click(&mut rt, "nav-home");
    rt.update(|s, _| live_mut(s).app.home_select_general());
    advance(&mut rt, 0.2);
    assert!(rt.find(&format!("live-session-{index}")).is_none());

    rt.update(|s, _| live_mut(s).app.home_select_project(target.clone()));
    advance(&mut rt, 0.2);
    assert!(rt.find(&format!("live-session-{index}")).is_some());
    click(&mut rt, &format!("live-session-{index}"));
    advance(&mut rt, 0.2);
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    assert_eq!(live(&rt.state).app.selected, Some(index));
}

#[test]
fn home_model_picker_writes_home_options_without_changing_an_existing_session() {
    let mut rt = design_runtime("home", (1280.0, 1000.0));
    let options = live(&rt.state).app.sessions[0].options.clone();
    click(&mut rt, "home-model-trigger");
    fill(&mut rt, "model-search", "Test model");
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        live(&rt.state).app.home.draft.options.model.as_deref(),
        Some("Test model")
    );
    assert_eq!(live(&rt.state).app.sessions[0].options, options);
    assert!(element(&rt, "home-draft").focused);
}

#[test]
fn fresh_launch_restores_home_without_background_results_stealing_navigation() {
    let saved = crate::evaluation::design()
        .live
        .as_ref()
        .unwrap()
        .app
        .snapshot();
    let mut s = crate::home::workspace(None);
    let l = live_mut(&mut s);
    l.app.enable_store("/unused-home-test".into());
    let id = l.app.commands()[0].0;
    let mut saved = saved;
    saved.home.text = "Saved home draft".into();
    l.apply(Event::Completed(
        id,
        Ok(Reply::Snapshot(Some(Box::new(saved)))),
    ));
    assert_eq!(l.panels.screen, Screen::Home);
    assert_eq!(l.home.editor.text, "Saved home draft");
    assert_eq!(s.title(), "Home");
    assert_eq!(
        crate::home::workspace(Some("/explicit".into()))
            .live
            .unwrap()
            .panels
            .screen,
        Screen::Conversation
    );
}

#[test]
fn home_layout_fits_compact_and_large_windows_and_settles_when_idle() {
    for size in [(800.0, 620.0), (1280.0, 1000.0)] {
        for scene in ["home", "home-empty", "home-draft"] {
            let app = spec();
            let mut state = crate::evaluation::design();
            assert!(crate::home::scene(&mut state, scene));
            let (send, receive) = std::sync::mpsc::channel();
            live_mut(&mut state).media = crate::media::State::new(std::sync::Arc::new(move || {
                let _ = send.send(());
            }));
            let mut rt = Runtime::new(state, app.view, size, Env::new(true, 1.0));
            rt.poll = app.poll;
            advance(&mut rt, 0.4);

            for _ in 0..2 {
                receive
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("both Home background decoders must notify completion");
            }
            advance(&mut rt, 1.0);
            for key in [
                "home-heading",
                "home-composer",
                "home-editor",
                "home-send",
                "home-project",
            ] {
                let e = element(&rt, key);
                assert!(
                    e.rect
                        .iter()
                        .zip(e.visible)
                        .all(|(a, b)| (*a - b).abs() < 1.0),
                    "{scene} {size:?} {key}: {e:?}"
                );
            }
            assert!(element(&rt, "home-composer").rect[2] <= 640.1);
            assert!(rt.find("message-composer").is_none());
            let commits = rt.stats.commits;
            let background = live(&rt.state).home.background.as_ref().unwrap();
            let images = (background.dark.id(), background.light.id());
            advance(&mut rt, 1.0);
            assert_eq!(rt.stats.commits, commits, "{scene} {size:?}");
            let background = live(&rt.state).home.background.as_ref().unwrap();
            assert_eq!((background.dark.id(), background.light.id()), images);
        }
    }
}

#[test]
fn background_streaming_on_home_updates_sessions_without_rebuilding_or_taking_focus() {
    let mut rt = design_runtime("home", (1280.0, 1000.0));
    fill(&mut rt, "home-draft", "Keep writing here");
    rt.update(|s, _| {
        let l = live_mut(s);
        l.app.sessions[0].remote_id = Some("background".into());
        l.app.sessions[0].conversation.set_working(true);
        l.app.sessions[0].last_activity = Some(1);
    });
    advance(&mut rt, 0.3);
    let l = live_mut(&mut rt.state);
    let delta = || {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/projects/caelum".into(),
            method: "item/agentMessage/delta".into(),
            params: serde_json::json!({"threadId":"background","itemId":"background-answer","delta":"More output","_created":1}),
            request_id: None,
        })
    };
    l.apply_events([delta()]);
    assert!(!l.apply_events([delta(), delta()]));
    assert_eq!(l.panels.screen, Screen::Home);
    assert_eq!(l.home.editor.text, "Keep writing here");
    assert!(l.home.editor.focused);
    assert!(
        l.app.sessions[0]
            .conversation
            .entries
            .iter()
            .any(|e| e.text.contains("More outputMore outputMore output"))
    );
}

#[test]
fn home_editor_restores_long_draft_scroll_after_navigation() {
    let mut rt = design_runtime("home", (1280.0, 1000.0));
    fill(
        &mut rt,
        "home-draft",
        &"An independent café draft\n".repeat(80),
    );
    rt.update(|_, tx| tx.scroll_to_y("home-editor", 240.0));
    advance(&mut rt, 0.3);
    let offset = element(&rt, "home-editor").rect[1] - element(&rt, "home-draft").rect[1];
    assert!(offset > 200.0);
    let project = live(&rt.state).app.sessions[0].scope.project.clone();
    rt.update(|s, _| live_mut(s).app.home_select_project(project.clone()));
    advance(&mut rt, 0.1);
    click(&mut rt, "live-session-0");
    click(&mut rt, "nav-home");
    advance(&mut rt, 0.3);
    let restored = element(&rt, "home-editor").rect[1] - element(&rt, "home-draft").rect[1];
    assert!((offset - restored).abs() < 1.0, "{offset} -> {restored}");
}

#[test]
fn home_background_presets_decode_swap_and_persist() {
    let mut rt = design_runtime("home", (1280.0, 1000.0));
    assert_eq!(
        live(&rt.state).home.background.as_ref().unwrap().id,
        crate::home::backgrounds::DEFAULT
    );
    assert!(rt.find("home-ascii").is_some());

    for preset in crate::home::backgrounds::PRESETS {
        rt.update(|s, _| {
            live_mut(s).app.preferences.home_background = preset.id.into();
        });
        advance(&mut rt, 0.2);
        let bg = live(&rt.state).home.background.as_ref().unwrap();
        assert_eq!(bg.id, preset.id);
        assert!(
            bg.dark.error().is_none(),
            "preset {} failed dark decode: {:?}",
            preset.id,
            bg.dark.error()
        );
        assert!(
            bg.light.error().is_none(),
            "preset {} failed light decode: {:?}",
            preset.id,
            bg.light.error()
        );
        assert_eq!(
            rt.find("home-ascii").is_some(),
            preset.id == crate::home::backgrounds::DEFAULT
        );
    }

    rt.update(|s, _| {
        live_mut(s).app.preferences.home_background = "unknown-preset".into();
    });
    advance(&mut rt, 0.2);
    assert_eq!(
        live(&rt.state).home.background.as_ref().unwrap().id,
        crate::home::backgrounds::DEFAULT
    );

    rt.update(|s, _| {
        live_mut(s).app.preferences.home_background = "starry-columns".into();
    });
    let snapshot: caelum_core::application::Snapshot =
        serde_json::from_slice(&serde_json::to_vec(&live(&rt.state).app.snapshot()).unwrap())
            .unwrap();
    assert_eq!(snapshot.preferences.home_background, "starry-columns");
}
