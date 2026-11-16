use super::*;
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
    use caelum_core::application::presentation::ActivityKind;
    let families = spec().families;
    let _fonts = rune::text::with_families(families.0, families.1);
    let mut rt = fixture((800.0, 620.0), "running");
    let state = &mut rt.state;
    let activity = |state: &Workspace| crate::activity::session_activity(state, 0).map(|a| a.kind);
    state.live.as_mut().unwrap().app.sessions[0].submitting = true;
    assert_eq!(activity(state), Some(ActivityKind::Working));
    state.live.as_mut().unwrap().app.sessions[0].submitting = false;
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .working = true;
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .retrying = true;
    assert_eq!(activity(state), Some(ActivityKind::Retrying));
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .retrying = false;
    state.live.as_mut().unwrap().app.sessions[0]
        .conversation
        .apply(
            Provider::OpenCode,
            "session.compaction.started",
            &json!({"inputID":"msg_compact", "_created": 1000}),
        );
    assert_eq!(activity(state), Some(ActivityKind::Working));
    state.live.as_mut().unwrap().app.sessions[0].stopping = true;
    assert_eq!(activity(state), Some(ActivityKind::Stopping));
    state.live.as_mut().unwrap().app.sessions[0].disconnected = true;
    assert!(activity(state).is_none());
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
