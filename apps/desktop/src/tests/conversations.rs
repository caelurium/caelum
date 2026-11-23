use super::*;
use crate::{
    live::{live, live_mut},
    panels::Screen,
};
use caelum_core::application::{Command, Entry, EntryKind, Event};

fn overview_shortcut(rt: &mut Runtime<Workspace>) {
    key(
        rt,
        Key::Command('j'),
        Mods {
            cmd: true,
            ..Default::default()
        },
    );
    advance(rt, 0.4);
}

fn tile_shows(rt: &Runtime<Workspace>, key: &str, content: &str) -> bool {
    let path = element(rt, key).path;
    rt.elements().iter().any(|e| {
        !e.exiting
            && e.path.starts_with(&path)
            && e.text.as_deref().is_some_and(|t| t.contains(content))
    })
}

#[test]
fn overview_preserves_the_reading_place_and_native_draft_through_escape_and_switches() {
    let mut rt = design_runtime("session-feed", (800.0, 620.0));
    rt.update(|s, _| {
        let sessions = &mut live_mut(s).app.sessions;
        sessions[1].conversation.set_working(true);
        let session = &mut sessions[0];
        session.conversation.entries = (0..60)
            .map(|i| {
                Entry::new(
                    format!("note-{i}"),
                    EntryKind::Assistant,
                    format!("Notebook entry {i}"),
                    true,
                )
            })
            .collect();
        session.conversation.set_working(true);
    });
    advance(&mut rt, 0.4);
    fill(&mut rt, "live-draft-0", "Keep this café draft");
    rt.update(|s, tx| {
        s.live.as_mut().unwrap().drafts[0].set_selection(5, 9);
        tx.scroll_center_in_instant("live-rows-0", "live-entry-0-note-25");
    });
    advance(&mut rt, 0.5);
    let position = live(&rt.state).reading_position(0).offset[1];
    assert!(position > 100.0);
    let reading_line = "live-entry-0-note-25";
    let before_open = element(&rt, reading_line).rect[1];
    overview_shortcut(&mut rt);
    assert_eq!(live(&rt.state).panels.screen, Screen::Overview);
    assert!(element(&rt, "overview-search").focused);
    rt.paste("not here");
    advance(&mut rt, 0.15);
    assert!(rt.find("overview-session-0").is_none());
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.35);
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    assert_eq!(live(&rt.state).drafts[0].selected(), "this");
    assert!(!live(&rt.state).app.sessions[0].stopping());
    assert!(element(&rt, "live-draft-0").focused);
    let after_escape = element(&rt, reading_line).rect;
    assert!(
        (after_escape[1] - before_open).abs() < 2.0,
        "after escape expected y {before_open}, got {after_escape:?}"
    );

    overview_shortcut(&mut rt);
    click(&mut rt, "overview-session-1");
    assert_eq!(live(&rt.state).app.selected, Some(1));
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    overview_shortcut(&mut rt);
    click(&mut rt, "overview-session-0");
    advance(&mut rt, 0.5);
    assert!(
        rt.find(reading_line).is_some(),
        "lost the visible message at {position}; now {:?}",
        live(&rt.state).reading_position(0)
    );
    assert!(
        (element(&rt, reading_line).rect[1] - before_open).abs() < 2.0,
        "expected y {before_open}, got {:?}",
        element(&rt, reading_line).rect
    );
    let l = rt.state.live.as_mut().unwrap();
    assert_eq!(l.app.selected, Some(0));
    assert_eq!(l.drafts[0].text, "Keep this café draft");
    assert_eq!(l.drafts[0].selected(), "this");
    assert!(!l.app.commands().iter().any(|(_, c)| matches!(
        c,
        Command::Start(..) | Command::Send(..) | Command::Cancel(_)
    )));
}

#[test]
fn overview_groups_sessions_and_supports_keyboard_navigation_and_tabs() {
    for size in [(800.0, 620.0), (1280.0, 1000.0)] {
        let mut rt = design_runtime("conversation-overview", size);
        assert_eq!(live(&rt.state).panels.screen, Screen::Overview);
        assert!(rt.find("overview-card-activity").is_some());
        assert!(rt.find("overview-card-machines").is_some());
        assert!(rt.find("overview-group-Waiting").is_some());
        assert!(rt.find("overview-group-Running").is_some());
        assert!(rt.find("overview-group-Settled").is_some());
        assert!(tile_shows(
            &rt,
            "overview-session-1",
            "Should a manual disconnect"
        ));
        assert!(tile_shows(
            &rt,
            "overview-session-3",
            "tracing where the editor loses focus"
        ));
        assert!(tile_shows(&rt, "overview-session-3", "Running · 3m"));
        assert!(tile_shows(
            &rt,
            "overview-session-4",
            "Allow the agent to start the preview"
        ));
        assert!(tile_shows(
            &rt,
            "overview-session-5",
            "Compare the cold and incremental"
        ));

        click(&mut rt, "overview-tab-prs");
        advance(&mut rt, 0.2);
        assert!(rt.find("overview-prs").is_some());
        click(&mut rt, "overview-tab-worktrees");
        advance(&mut rt, 0.2);
        assert!(
            rt.find("overview-worktrees").is_some()
                || tile_shows(&rt, "conversation-overview", "No worktrees loaded")
        );
        click(&mut rt, "overview-tab-sessions");
        advance(&mut rt, 0.2);

        click(&mut rt, "overview-requests");
        key(&mut rt, Key::Down, Mods::default());
        key(&mut rt, Key::Enter, Mods::default());
        advance(&mut rt, 0.3);
        assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
        assert!(live(&rt.state).app.selected.is_some());
    }
}

#[test]
fn overview_streams_visible_excerpts_and_keeps_filtered_updates_for_when_the_row_returns() {
    let mut rt = design_runtime("conversation-overview", (1280.0, 1000.0));
    let card = "overview-session-3";
    let delta = |id: &str, text: &str| {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/projects/rune".into(),
            method: "item/agentMessage/delta".into(),
            params: serde_json::json!({"threadId":"fixture-3","itemId":id,"delta":text}),
            request_id: None,
        })
    };
    rt.update(|s, _| assert!(live_mut(s).apply_events([delta("step", "Checking the editor")])));
    advance(&mut rt, 0.1);
    rt.update(|s, _| assert!(live_mut(s).apply_events([delta("step", " after sleep.")])));
    advance(&mut rt, 0.1);
    assert!(tile_shows(&rt, card, "Checking the editor after sleep."));
    rt.update(|s, _| {
        let l = live_mut(s);
        assert!(l.apply_events([delta("step", &" progress".repeat(40))]));
        assert!(!l.apply_events([delta("step", " beyond the excerpt")]));
    });
    fill(&mut rt, "overview-search", "Reconnect");
    assert!(rt.find(card).is_none());
    rt.update(|s, _| {
        assert!(!live_mut(s).apply_events([delta("final-step", "Found the focus change.")]))
    });
    fill(&mut rt, "overview-search", "keyboard focus");
    assert!(tile_shows(&rt, card, "Found the focus change."));
    let now = live(&rt.state).feed.displayed_at();
    rt.update(|s, tx| {
        let l = live_mut(s);
        assert!(!l.overview.tick(now + 59_999, tx));
        l.feed.set_display_clock(now + 60_000);
        assert!(l.overview.tick(now + 60_000, tx));
    });
    advance(&mut rt, 0.1);
    assert!(tile_shows(&rt, card, "Running · 4m"));
    fill(&mut rt, "overview-search", "absent");
    rt.update(|s, tx| assert!(!live_mut(s).overview.tick(now + 120_000, tx)));
}

#[test]
fn overview_moves_background_sessions_between_running_and_settled_live() {
    let mut rt = design_runtime("session-feed", (1280.0, 1000.0));
    overview_shortcut(&mut rt);
    assert!(rt.find("overview-group-Running").is_none());
    assert!(rt.find("overview-group-Settled").is_some());
    fill(&mut rt, "overview-search", "keyboard focus");
    assert!(rt.find("overview-session-3").is_some());
    let event = |method: &str| {
        Event::Codex(caelum_protocol::CodexEvent {
            project_path: "/projects/rune".into(),
            method: method.into(),
            params: serde_json::json!({"threadId":"fixture-3", "turn":{"id":"turn"}}),
            request_id: None,
        })
    };
    rt.update(|s, _| assert!(live_mut(s).apply_events([event("turn/started")])));
    advance(&mut rt, 0.25);
    assert!(rt.find("overview-group-Running").is_some());
    assert!(tile_shows(&rt, "overview-session-3", "Running"));
    rt.update(|s, _| assert!(live_mut(s).apply_events([event("turn/completed")])));
    advance(&mut rt, 0.25);
    assert!(rt.find("overview-group-Running").is_none());
    assert!(rt.find("overview-group-Settled").is_some());
    assert!(rt.find("overview-session-3").is_some());
}

#[test]
fn overview_windows_large_projects_and_finds_sessions_outside_the_sidebar_filter() {
    let mut rt = design_runtime("session-feed-many", (800.0, 620.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.preferences.session_scope = caelum_core::application::SessionScope::CurrentProject;
        l.panels.sessions.replace_text("no match");
        for session in l.app.sessions.iter_mut().filter(|s| s.title.ends_with('9')) {
            session.conversation.set_working(true);
        }
        assert_eq!(l.app.overview_sessions("").len(), 200);
    });
    click(&mut rt, "nav-overview");
    advance(&mut rt, 0.4);
    let mounted = rt
        .elements()
        .into_iter()
        .filter(|e| {
            !e.exiting
                && e.key
                    .as_deref()
                    .is_some_and(|k| k.starts_with("overview-session-"))
        })
        .count();
    assert!((1..64).contains(&mounted), "{mounted} mounted");
    fill(&mut rt, "overview-search", "Historical session 1999");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    assert_eq!(rt.state.title(), "Historical session 1999");
    assert_eq!(live(&rt.state).panels.sessions.text, "no match");
}

#[test]
fn overview_opens_the_scoped_vm_thread_and_preserves_its_draft() {
    let mut rt = design_runtime("", (1280.0, 1000.0));
    rt.update(|s, _| {
        crate::remote::scene(s, "remote-working");
        live_mut(s).app.sessions[0].conversation.set_working(true);
    });
    advance(&mut rt, 0.3);
    fill(
        &mut rt,
        "remote-draft-checkout-example",
        "Keep this VM draft",
    );
    overview_shortcut(&mut rt);
    let target = rt
        .elements()
        .into_iter()
        .find(|e| {
            !e.exiting
                && e.role == "button"
                && e.path.contains("conversation-overview")
                && e.label.as_deref() == Some("Add a health check and show me the changes.")
        })
        .expect("scoped VM row");
    let vm = target.key.unwrap();
    assert!(tile_shows(&rt, &vm, "dev-01"));
    assert!(tile_shows(&rt, &vm, "Running cargo"));
    click(&mut rt, "overview-session-0");
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    overview_shortcut(&mut rt);
    click(&mut rt, &vm);
    let l = live(&rt.state);
    assert_eq!(l.panels.screen, Screen::Remote);
    assert_eq!(
        l.app.remote.saved.selected.as_deref(),
        Some("checkout-example")
    );
    assert_eq!(
        element(&rt, "remote-draft-checkout-example")
            .text
            .as_deref(),
        Some("Keep this VM draft")
    );
}

#[test]
fn request_shortcut_leads_to_the_actual_question_and_the_title_browses_prompts() {
    let mut rt = design_runtime("conversation-overview", (1280., 1000.));
    click(&mut rt, "overview-requests");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    let selected = live(&rt.state).app.selected.unwrap();
    assert_eq!(live(&rt.state).panels.screen, Screen::Conversation);
    assert!(selected == 1 || selected == 4, "selected {selected}");
    click(&mut rt, "live-session-0");
    let before = live(&rt.state).drafts[0].text.clone();
    click(&mut rt, "conversation-heading");
    assert_eq!(live(&rt.state).panels.overlay(), Some(Screen::Timeline));
    assert_eq!(live(&rt.state).commands.owner, Some(0));
    assert_eq!(live(&rt.state).drafts[0].text, before);
}
