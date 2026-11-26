use super::*;
use caelum_core::application::{Event, Provider};
use serde_json::json;

fn failure() -> Event {
    Event::OpenCode(caelum_protocol::OpenCodeEvent {
        project_path: "/project".into(),
        event_type: "session.execution.failed".into(),
        data: json!({"sessionID":"example", "error":{
            "type":"provider.transport", "status":200,
            "message":"Timed out waiting for response data"}}),
        id: None,
        created: Some(100),
        location: None,
    })
}

#[test]
fn provider_errors_are_readable_copyable_and_dismissible_without_changing_the_draft() {
    for dark in [true, false] {
        let mut rt = live_runtime_size((800.0, 620.0));
        let clipboard = rune::clipboard::memory();
        rt.use_clipboard(&clipboard);
        rt.set_system_dark(dark);
        rt.update(|s, _| {
            let app = &mut s.live.as_mut().unwrap().app;
            app.sessions[0].scope.provider = Provider::OpenCode;
            app.sessions[0].remote_id = Some("example".into());
        });
        fill(&mut rt, "live-draft-0", "Keep this café draft");
        let caret = rt.state.live.as_ref().unwrap().drafts[0].focus;
        rt.update(|s, _| s.live.as_mut().unwrap().apply(failure()));
        advance(&mut rt, 0.6);
        reveal(&mut rt, "provider-error-0");
        for control in [
            "provider-error-0",
            "copy-provider-error-0",
            "dismiss-provider-error-0",
        ] {
            let e = element(&rt, control);
            assert!(
                e.visible[2] >= e.rect[2] - 1.0 && e.visible[3] >= e.rect[3] - 1.0,
                "{e:?}"
            );
        }
        let labels: Vec<_> = rt
            .elements()
            .iter()
            .filter_map(|e| e.text.clone())
            .collect();
        assert!(
            labels
                .iter()
                .any(|label| label == "Timed out waiting for response data")
        );
        assert!(
            !labels
                .iter()
                .any(|label| label.contains("provider.transport"))
        );
        click(&mut rt, "copy-provider-error-0");
        let copied: serde_json::Value = serde_json::from_str(&clipboard.text().unwrap()).unwrap();
        assert_eq!(copied["type"], "provider.transport");
        click(&mut rt, "dismiss-provider-error-0");
        assert!(rt.find("provider-error-0").is_none());
        let l = rt.state.live.as_ref().unwrap();
        assert_eq!(l.drafts[0].text, "Keep this café draft");
        assert_eq!(l.drafts[0].focus, caret);
        assert!(l.app.sessions[0].queue_paused);
        click(&mut rt, "live-draft-0");
        key(
            &mut rt,
            Key::Command('z'),
            Mods {
                cmd: true,
                ..Default::default()
            },
        );
        assert_eq!(rt.state.live.as_ref().unwrap().drafts[0].text, "");
    }
}

#[test]
fn recovery_in_a_background_session_refreshes_its_error_indicator() {
    let mut rt = live_runtime();
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        app.sessions[1].scope.provider = Provider::OpenCode;
        app.sessions[1].remote_id = Some("example".into());
        app.apply(failure());
    });
    advance(&mut rt, 0.1);
    assert!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .conversation
            .failure
            .notice()
            .is_some()
    );
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let mut event = failure();
        if let Event::OpenCode(e) = &mut event {
            e.event_type = "session.text.delta".into();
            e.created = Some(200);
            e.data =
                json!({"sessionID":"example", "assistantMessageID":"reply", "delta":"Recovered"});
        }
        assert!(l.apply_events([event]));
        assert!(l.app.sessions[1].conversation.failure.notice().is_none());
        assert_eq!(l.app.selected, Some(0));
    });
}

#[test]
fn session_error_notice_is_dismissible_without_changing_the_draft() {
    let mut rt = live_runtime_size((800.0, 620.0));
    fill(&mut rt, "live-draft-0", "Keep this draft");
    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0].error = Some("Unknown command /usage".into());
    });
    advance(&mut rt, 0.2);
    reveal(&mut rt, "session-error-0");
    for control in ["session-error-0", "dismiss-session-error-0"] {
        let e = element(&rt, control);
        assert!(
            e.visible[2] >= e.rect[2] - 1.0 && e.visible[3] >= e.rect[3] - 1.0,
            "{e:?}"
        );
    }
    click(&mut rt, "dismiss-session-error-0");
    assert!(rt.find("session-error-0").is_none());
    let l = rt.state.live.as_ref().unwrap();
    assert!(l.app.sessions[0].error.is_none());
    assert_eq!(l.drafts[0].text, "Keep this draft");
}
