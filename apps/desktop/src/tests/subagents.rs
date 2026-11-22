use super::*;
use crate::live;
use caelum_core::application::{Command, Event, Provider};
use serde_json::json;

fn fixture(size: (f32, f32), status: &str) -> Runtime<Workspace> {
    let mut rt = live_runtime_size(size);
    rt.update(|s, _| {
        s.live.as_mut().unwrap().fixture = true;
        crate::subagents::scene(s, status);
    });
    advance(&mut rt, 0.3);
    rt
}

#[test]
fn tasks_show_named_progress_complete_in_place_and_copy_prompt_and_result() {
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    for size in [(800.0, 620.0), (1280.0, 1000.0)] {
        let mut rt = fixture(size, "running");
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        let key = "subagent-0-opencode-tool-explore";
        assert!(element(&rt, key).visible[3] > 0.0);
        assert_eq!(element(&rt, key).expanded, Some(true));
        assert_eq!(
            element(&rt, "session-activity").label.as_deref(),
            Some("1 subagent running")
        );
        assert_eq!(
            element(&rt, "session-waiting-0").label.as_deref(),
            Some("1 subagent running")
        );
        assert!(
            rt.elements()
                .iter()
                .any(|e| !e.exiting && e.text.as_deref() == Some("Subagent · explore"))
        );
        click(&mut rt, "subagent-Prompt-0-opencode-tool-explore-copy");
        assert!(clipboard.text().unwrap().starts_with("Trace the request"));
        click(&mut rt, key);
        assert_eq!(element(&rt, key).expanded, Some(false));
        rt.update(|s, _| s.live.as_mut().unwrap().apply(Event::OpenCode(caelum_protocol::OpenCodeEvent {
            project_path: "/project".into(), event_type: "session.synthetic".into(),
            id: Some("evt_report".into()), created: Some(caelum_core::application::presentation::now()), location: None,
            data: json!({"sessionID":"ses_parent", "description":"Map request flow",
                "metadata":{"source":"subagent","childID":"ses_explore","agent":"explore","state":"completed"},
                "text":"<subagent sessionID=\"ses_explore\" state=\"completed\" description=\"Map request flow\">\nFound the café handler.\n</subagent>"}),
        })));
        advance(&mut rt, 0.3);
        assert_eq!(element(&rt, key).expanded, Some(false));
        assert!(rt.find("session-activity").is_none());
        assert!(rt.find("session-waiting-0").is_none());
        assert!(
            rt.elements()
                .iter()
                .any(|e| !e.exiting && e.text.as_deref() == Some("Completed"))
        );
        click(&mut rt, key);
        reveal(&mut rt, "subagent-Result-0-opencode-tool-explore-copy");
        click(&mut rt, "subagent-Result-0-opencode-tool-explore-copy");
        assert_eq!(
            clipboard.text().unwrap().as_str(),
            "Found the café handler."
        );
        assert!(!rt.elements().iter().any(|e| {
            !e.exiting
                && e.text
                    .as_deref()
                    .is_some_and(|text| text.contains("<subagent") || text.contains("ses_explore"))
        }));
        assert_eq!(
            rt.state.live.as_ref().unwrap().app.sessions[0]
                .conversation
                .entries
                .len(),
            2
        );
    }
}

#[test]
fn parent_activity_takes_precedence_while_subagents_are_running() {
    use caelum_core::application::{Command, Event, Reply, presentation::ActivityKind};
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    let mut rt = fixture((800.0, 620.0), "running");
    let state = &mut rt.state;
    let activity = |state: &Workspace| crate::activity::session_activity(state, 0).map(|a| a.kind);
    assert!(
        state
            .live
            .as_mut()
            .unwrap()
            .app
            .send(0, "Start turn".into())
    );
    assert_eq!(activity(state), Some(ActivityKind::Working));
    {
        let app = &mut state.live.as_mut().unwrap().app;
        let request = app
            .commands()
            .into_iter()
            .find(|(_, command)| matches!(command, Command::Send(..)))
            .unwrap()
            .0;
        app.apply(Event::Completed(request, Ok(Reply::Done)));
    }
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .set_working(true);
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .set_retrying(true);
    assert_eq!(activity(state), Some(ActivityKind::Retrying));
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .set_retrying(false);
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .apply(
            Provider::OpenCode,
            "session.compaction.started",
            &json!({"inputID":"msg_compact", "_created": 1000}),
        );
    assert_eq!(activity(state), Some(ActivityKind::Working));
    state.live.as_mut().unwrap().app.cancel(0);
    assert_eq!(activity(state), Some(ActivityKind::Stopping));
    state.live.as_mut().unwrap().app.sessions[0].disconnected = true;
    assert!(activity(state).is_none());
}

#[test]
fn peeking_loads_the_child_without_navigating_and_shows_its_own_question() {
    let mut rt = fixture((800.0, 620.0), "running");
    let header = "subagent-0-opencode-tool-explore";
    click(&mut rt, header);
    click(&mut rt, header);
    let child = live::live(&rt.state)
        .app
        .sessions
        .iter()
        .position(|session| session.remote_id.as_deref() == Some("ses_explore"))
        .unwrap();
    let delta = || {
        Event::OpenCode(caelum_protocol::OpenCodeEvent {
            project_path: "/project".into(),
            event_type: "session.text.delta".into(),
            data: json!({"sessionID":"ses_explore","assistantMessageID":"progress","delta":"Checking retries. ","_created":1000}),
            id: None,
            created: Some(1000),
            location: None,
        })
    };
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        assert!(l.apply_events([delta()]));
        assert!(l.apply_events([delta()]), "visible child text changed");
    });
    advance(&mut rt, 0.1);
    assert!(rt.elements().iter().any(|e| !e.exiting && e.text.as_deref() == Some("Checking retries. Checking retries.")));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let session = &l.app.sessions[child];
        l.apply(Event::Questions(vec![caelum_protocol::QuestionPrompt {
            provider: session.scope.provider.as_str().into(),
            project_path: session.scope.project.clone(),
            session_id: "ses_explore".into(),
            request_id: "child-question".into(),
            title: "Should a manual disconnect stop retries?".into(),
            fields: vec![],
        }]));
    });
    advance(&mut rt, 0.2);
    assert_eq!(live::live(&rt.state).app.selected, Some(0));
    assert!(rt.elements().iter().any(
        |e| !e.exiting && e.text.as_deref() == Some("Should a manual disconnect stop retries?")
    ));
    assert!(rt.accessibility_action(header, rune::accesskit::Action::Focus, None));
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.25);
    assert_eq!(element(&rt, header).expanded, Some(false));
    rt.update(|s, _| assert!(!s.live.as_mut().unwrap().apply_events([delta()])));
    let app = &live::live(&rt.state).app;
    assert_eq!(app.selected, Some(0));
    assert!(!app.sessions[0].stopping());
}

#[test]
fn opening_and_returning_to_a_child_preserves_both_native_editors() {
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    let mut rt = fixture((800.0, 620.0), "completed");
    fill(&mut rt, "live-draft-0", "Parent café draft");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[1].scope.provider = Provider::OpenCode;
        l.app.sessions[1].remote_id = Some("ses_explore".into());
        l.app.sessions[1].draft = "Child draft".into();
        l.drafts[1].replace_text("Child draft");
        l.app.commands();
    });
    click(&mut rt, "subagent-open-0-opencode-tool-explore");
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(1));
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[1].text,
        "Child draft"
    );
    click(&mut rt, "subagent-parent");
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(0));
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[0].text,
        "Parent café draft"
    );
    assert!(
        !rt.state
            .live
            .as_mut()
            .unwrap()
            .app
            .commands()
            .iter()
            .any(|(_, command)| matches!(command, Command::Start(..) | Command::Send(..)))
    );
}

#[test]
fn failed_results_and_long_text_stay_readable_and_bounded_in_compact_layouts() {
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    let mut rt = fixture((800.0, 620.0), "error");
    assert!(
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.text.as_deref() == Some("Failed"))
    );
    let output = "Long café result\n".repeat(2000);
    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .entries[1]
            .output
            .clone_from(&output);
    });
    advance(&mut rt, 0.3);
    assert!(element(&rt, "subagent-Result-0-opencode-tool-explore-scroll").rect[3] <= 220.1);
    assert!(element(&rt, "composer-editor").visible[3] >= 46.0);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    reveal(&mut rt, "subagent-Result-0-opencode-tool-explore-copy");
    click(&mut rt, "subagent-Result-0-opencode-tool-explore-copy");
    assert_eq!(clipboard.text().unwrap().as_str(), output);
}
