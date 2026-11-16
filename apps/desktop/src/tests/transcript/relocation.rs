use super::*;
use caelum_core::application::{Command, Event, Provider, Reply};
use caelum_protocol::ProjectInfo;
use serde_json::json;

#[test]
fn moved_session_keeps_native_draft_selection_undo_and_reading_position() {
    let mut rt = live_runtime_size((800.0, 620.0));
    rt.update(|s, _| {
        let live = s.live.as_mut().unwrap();
        live.fixture = true;
        let session = &mut live.app.sessions[0];
        session.scope.provider = Provider::OpenCode;
        session.remote_id = Some("ses_moved".into());
        session.conversation.history(Provider::OpenCode, &(0..60).map(|n| {
            json!({"id":format!("prompt-{n}"), "type":"user", "text":format!("Prompt {n}: keep the reading position")})
        }).collect::<Vec<_>>(), false);
    });
    advance(&mut rt, 0.6);
    fill(&mut rt, "live-draft-0", "Keep this café draft");
    rt.update(|s, tx| {
        s.live.as_mut().unwrap().drafts[0].set_selection(2, 8);
        tx.scroll_center_in_instant("live-rows-0", "live-entry-0-opencode-prompt-25");
    });
    advance(&mut rt, 0.3);
    let anchor = element(&rt, "live-entry-0-opencode-prompt-25").rect[1];
    assert_eq!(rt.scroll_at_bottom("live-transcript-0"), Some(false));
    rt.update(|s, _| s.live.as_mut().unwrap().app.refresh(0));
    let (request, from) = rt
        .state
        .live
        .as_mut()
        .unwrap()
        .app
        .commands()
        .into_iter()
        .find_map(|(id, command)| {
            if let Command::Resume(key) = command {
                Some((id, key))
            } else {
                None
            }
        })
        .unwrap();
    rt.update(|s, _| {
        s.live.as_mut().unwrap().apply(Event::Completed(
            request,
            Ok(Reply::SessionMoved {
                from,
                project: ProjectInfo {
                    path: "/linked".into(),
                    name: "Linked checkout".into(),
                },
            }),
        ));
    });
    advance(&mut rt, 0.3);
    let live = rt.state.live.as_ref().unwrap();
    assert_eq!(live.app.selected, Some(0));
    assert_eq!(live.app.active_project.as_deref(), Some("/linked"));
    assert_eq!(live.drafts[0].text, "Keep this café draft");
    assert_eq!((live.drafts[0].anchor, live.drafts[0].focus), (2, 8));
    // Loading changes the header's height; the visible row, rather than its
    // absolute scroll offset, must remain anchored on screen.
    assert!((element(&rt, "live-entry-0-opencode-prompt-25").rect[1] - anchor).abs() < 1.0);
    assert_eq!(rt.scroll_at_bottom("live-transcript-0"), Some(false));
    key(
        &mut rt,
        Key::Command('z'),
        Mods {
            cmd: true,
            ..Default::default()
        },
    );
    assert!(rt.state.live.as_ref().unwrap().drafts[0].text.is_empty());
}
