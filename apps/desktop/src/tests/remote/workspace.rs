use super::*;
use crate::live::{live, live_mut};
use caelum_core::remote::{Line, Page, RunStatus, Speaker, ToolState, TurnEnd};

#[test]
fn composer_tracks_the_exact_run_through_queue_tool_stream_stop_and_completion() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-working"));
    advance(&mut rt, 0.2);
    let id = "checkout-example";
    let status = |rt: &Runtime<Workspace>| element(rt, "remote-turn-status").label.unwrap();
    assert_eq!(status(&rt), "Running cargo…");
    assert!(
        element(&rt, "remote-destination")
            .text
            .as_deref()
            .unwrap()
            .contains("dev-01")
    );
    for (phase, lines, expected) in [
        (RunStatus::Queued, vec![], "Queued on Boat…"),
        (
            RunStatus::Running,
            vec![Line {
                partial: true,
                ..Line::new(Speaker::OpenCode, "Here is the response")
            }],
            "Responding…",
        ),
    ] {
        rt.update(|s, _| {
            let l = live_mut(s);
            l.app.follow_remote(id);
            let (r, _) = l
                .app
                .commands()
                .into_iter()
                .find(|(_, op)| matches!(op, Command::Remote(Operation::Follow(..))))
                .unwrap();
            l.apply(Event::Completed(
                r,
                Ok(Reply::Remote(Outcome::Page(Page {
                    cursor: Some("next".into()),
                    lines,
                    more: false,
                    status: phase,
                }))),
            ));
        });
        advance(&mut rt, 0.1);
        assert_eq!(status(&rt), expected);
    }
    rt.update(|s, _| {
        let l = live_mut(s);
        l.app.follow_remote(id);
        let (r, _) = l.app.commands().pop().unwrap();
        l.apply(Event::Completed(r, Err("Connection lost".into())));
    });
    advance(&mut rt, 0.1);
    assert_eq!(status(&rt), "Live updates paused");
    click(&mut rt, "remote-reconnect");
    rt.update(|s, _| {
        let l = live_mut(s);
        let commands = l.app.commands();
        let [(r, Command::Remote(Operation::Follow(_, run, cursor)))] = commands.as_slice() else {
            panic!("{commands:?}")
        };
        assert_eq!(
            (run.prompt.as_str(), cursor.as_deref()),
            ("prompt-example", Some("next"))
        );
        l.apply(Event::Completed(
            *r,
            Ok(Reply::Remote(Outcome::Page(Page {
                lines: vec![],
                cursor: cursor.clone(),
                more: false,
                status: RunStatus::Running,
            }))),
        ));
    });
    click(&mut rt, "remote-draft-checkout-example");
    key(&mut rt, Key::Escape, Mods::default());
    assert_eq!(status(&rt), "Stopping turn…");
    rt.update(|s, _| {
        let l = live_mut(s);
        let (r, Command::Remote(Operation::Interrupt(_, run))) = l.app.commands().pop().unwrap()
        else {
            panic!()
        };
        assert_eq!(run.conversation, "thread-example");
        l.apply(Event::Completed(r, Ok(Reply::Remote(Outcome::Interrupted))));
    });
    advance(&mut rt, 0.1);
    assert!(!element(&rt, "remote-stop").focusable && status(&rt) == "Stopping turn…");
    rt.update(|s, _| {
        let l = live_mut(s);
        let (r, _) = l.app.commands().pop().unwrap();
        l.apply(Event::Completed(
            r,
            Ok(Reply::Remote(Outcome::Page(Page {
                lines: vec![],
                cursor: None,
                more: false,
                status: RunStatus::Ended(TurnEnd::Interrupted),
            }))),
        ));
    });
    advance(&mut rt, 0.2);
    assert_eq!(status(&rt), "Ready");
    assert!(rt.find("remote-stop").is_none());
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Turn stopped"))
    );
    assert!(
        live(&rt.state).app.remote.saved.checkouts[id]
            .thread
            .lines
            .iter()
            .all(|l| !l.partial && l.speaker != Speaker::Tool(ToolState::Running))
    );
}
