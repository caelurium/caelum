use super::*;
use crate::{live::live, panels::Screen};
use caelum_core::application::Event;

#[test]
fn workspace_menu_routes_file_search_and_changes_without_losing_the_draft() {
    let mut rt = live_runtime_size((800.0, 620.0));
    fill(&mut rt, "live-draft-0", "Continue this draft");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.drafts[0].set_selection(0, 8);
        l.app.sessions[0].conversation.set_working(true);
    });
    assert!(rt.accessibility_action("workspace-trigger", rune::accesskit::Action::Focus, None));
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    advance(&mut rt, 0.2);
    assert!(element(&rt, "workspace-item0").focused);
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.2);
    assert!(element(&rt, "workspace-trigger").focused);
    assert!(!live(&rt.state).app.sessions[0].stopping());
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.4);
    assert_eq!(live(&rt.state).panels.inspector, Screen::Files);
    assert!(element(&rt, "file-search").focused);
    rt.paste("main.rs");
    advance(&mut rt, 0.1);
    assert_eq!(element(&rt, "file-search").text.as_deref(), Some("main.rs"));
    click(&mut rt, "workspace-trigger");
    key(&mut rt, Key::Home, Mods::default());
    key(&mut rt, Key::Down, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.2);
    assert_eq!(live(&rt.state).panels.inspector, Screen::Changes);
    let draft = &live(&rt.state).drafts[0];
    assert_eq!(draft.text, "Continue this draft");
    assert_eq!(draft.selected(), "Continue");
}

#[test]
fn session_feedback_opens_the_scoped_review_and_pending_input_takes_priority() {
    let mut rt = design_runtime("session-review-feedback", (800.0, 620.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[0].title = "Long session title ".repeat(15);
        l.drafts[0].set_selection(0, 3);
    });
    advance(&mut rt, 0.2);
    let link = element(&rt, "session-title-review");
    assert_eq!(link.label.as_deref(), Some("PR #95 · Changes requested"));
    assert!(link.focusable && link.visible[2] >= link.rect[2] - 1.0);
    assert!(link.rect[0] + link.rect[2] < element(&rt, "workspace-trigger").rect[0]);
    assert!(rt.accessibility_action("session-title-review", rune::accesskit::Action::Focus, None));
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    advance(&mut rt, 0.2);
    let l = live(&rt.state);
    assert_eq!(l.panels.screen, Screen::Review);
    assert_eq!(
        l.panels.review,
        Some((
            "/projects/caelum".into(),
            "https://github.com/example/caelum/pull/95".into()
        ))
    );
    assert!(rt.find("session-title-review").is_none());
    click(&mut rt, "back-to-session");
    assert_eq!(live(&rt.state).drafts[0].selected(), "Let");
    rt.update(|s, _| {
        s.live
            .as_mut()
            .unwrap()
            .apply(Event::Questions(vec![caelum_protocol::QuestionPrompt {
                provider: "codex".into(),
                project_path: "/projects/caelum".into(),
                session_id: "fixture".into(),
                request_id: "scope".into(),
                title: "Which platform?".into(),
                fields: vec![],
            }]));
    });
    advance(&mut rt, 0.2);
    assert_eq!(
        element(&rt, "session-title-input").text.as_deref(),
        Some("Needs input")
    );
    assert!(rt.find("session-title-review").is_none());
    click(&mut rt, "live-session-1");
    assert!(rt.find("session-title-input").is_none() && rt.find("session-title-review").is_none());
    assert_eq!(
        element(&rt, "session-input-0").text.as_deref(),
        Some("Needs input")
    );
    click(&mut rt, "live-session-0");
    rt.update(|s, _| s.live.as_mut().unwrap().apply(Event::Questions(vec![])));
    advance(&mut rt, 0.2);
    assert!(rt.find("session-title-review").is_some());
    assert!(rt.find("session-title-input").is_none());
}
