use super::*;
use crate::{
    live::{live, live_mut},
    panels::Screen,
};
use caelum_core::remote::{Job, Line, Page, RunStatus, Speaker, ToolState, TurnEnd};

#[test]
fn new_session_shortcuts_keep_the_chosen_vm_provider_and_checkout() {
    let mut rt = design_runtime("", (800., 620.));
    let id = "checkout-example";
    rt.update(|s, _| crate::remote::scene(s, "remote-machine"));
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Escape, Mods::default());
    fill(&mut rt, "remote-draft-checkout-example", "Park this draft");
    let shortcut = |rt: &mut Runtime<Workspace>, shift| {
        key(
            rt,
            Key::Command('n'),
            Mods {
                cmd: true,
                shift,
                ..Default::default()
            },
        );
        advance(rt, 0.2);
    };
    shortcut(&mut rt, true);
    assert_eq!(live(&rt.state).panels.launch_provider, Provider::OpenCode);
    fill(&mut rt, "launch-search", "no-such-machine");
    assert!(rt.find("launch-vm-checkout-example").is_none());
    fill(&mut rt, "launch-search", "feat/remote-checkout");
    click(&mut rt, "launch-provider-codex");
    click(&mut rt, "launch-vm-checkout-example");
    let l = live(&rt.state);
    assert_eq!(l.panels.screen, Screen::Remote);
    assert_eq!(l.app.remote.saved.selected.as_deref(), Some(id));
    let c = &l.app.remote.saved.checkouts[id];
    assert_eq!(c.thread.provider, Provider::Codex);
    assert_eq!(
        (
            c.other_threads[0].provider,
            c.other_threads[0].draft.as_str()
        ),
        (Provider::OpenCode, "Park this draft")
    );
    let first = c.thread.editor_key(id).to_owned();
    fill(&mut rt, &format!("remote-draft-{first}"), "Second draft");
    shortcut(&mut rt, false);
    let app = &mut live_mut(&mut rt.state).app;
    let c = &app.remote.saved.checkouts[id];
    assert!(c.thread.conversation.is_none() && c.thread.draft.is_empty());
    assert_ne!(c.thread.editor_key(id), first);
    assert_eq!(c.thread.provider, Provider::Codex);
    assert_eq!(c.other_threads[1].draft, "Second draft");
    assert!(
        app.commands()
            .iter()
            .all(|(_, op)| !matches!(op, Command::Remote(_)))
    );
}

#[test]
fn a_fresh_vm_session_parks_the_draft_and_only_starts_on_send() {
    let mut rt = design_runtime("", (800., 620.));
    let id = "checkout-example";
    rt.update(|s, _| crate::remote::scene(s, "remote-machine"));
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Escape, Mods::default());
    fill(&mut rt, "remote-draft-checkout-example", "Keep my draft");
    click(&mut rt, "vm-threads");
    click(&mut rt, "new-vm-session");
    let app = &mut live_mut(&mut rt.state).app;
    let c = &app.remote.saved.checkouts[id];
    let fresh = c.thread.editor_key(id).to_owned();
    assert_ne!(fresh, id);
    assert!(
        c.thread.conversation.is_none() && c.thread.lines.is_empty() && c.thread.draft.is_empty()
    );
    assert_eq!(c.other_threads[0].draft, "Keep my draft");
    assert!(
        app.commands()
            .iter()
            .all(|(_, op)| !matches!(op, Command::Remote(_)))
    );

    // Starting again on an untouched draft reuses the fresh editor.
    secondary(&mut rt, "remote-checkout-checkout-example");
    choose_context(&mut rt, "New session on this VM");
    let field = format!("remote-draft-{fresh}");
    fill(&mut rt, &field, "Independent task");
    click(&mut rt, "vm-provider-trigger");
    click(&mut rt, "option0");
    click(&mut rt, "vm-threads");
    click(&mut rt, "vm-thread-checkout-example");
    let c = &live(&rt.state).app.remote.saved.checkouts[id];
    assert_eq!(c.thread.conversation.as_deref(), Some("thread-example"));
    assert_eq!(c.thread.draft, "Keep my draft");
    assert_eq!(c.thread.provider, Provider::OpenCode);
    assert_eq!(c.other_threads[0].provider, Provider::Codex);
    assert_eq!(c.other_threads.len(), 1);
    click(&mut rt, "vm-threads");
    click(&mut rt, &format!("vm-thread-{fresh}"));
    click(&mut rt, "remote-send");
    let app = &mut live_mut(&mut rt.state).app;
    let commands = app.commands();
    assert!(
        matches!(commands.as_slice(), [(_, Command::Remote(Operation::Prompt(t, p)))] if t.machine.id == "vm-example" && p.provider == Provider::Codex && p.conversation.is_none() && p.model.is_none() && p.text == "Independent task")
    );
    assert!(!app.new_vm_session(id, Provider::Codex));
    assert!(!app.switch_vm_thread(id, id));
}

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

#[test]
fn project_vm_group_owns_its_worktree_rows_and_removal_preserves_other_checkouts() {
    let mut rt = design_runtime("", (1024., 800.));
    let id = "checkout-example";
    rt.update(|s, _| {
        crate::remote::scene(s, "remote-setup-failed");
        let l = live_mut(s);
        let mut other = l.app.remote.saved.checkouts[id].clone();
        let root = other.source.project.clone();
        other.source.project = "/synthetic/worktrees/topic".into();
        l.app
            .project_roots
            .insert(other.source.project.clone(), root);
        l.app.remote.saved.checkouts.insert("parked".into(), other);
        l.app
            .remote
            .jobs
            .insert("parked".into(), Job::Failed("Old error".into()));
    });
    advance(&mut rt, 0.2);
    let path = live(&rt.state).app.remote.saved.checkouts[id]
        .source
        .project
        .clone();
    let toggle = crate::code_view::source_key(&path, "remote-toggle");
    assert!(
        element(&rt, "remote-checkout-parked")
            .help
            .unwrap()
            .contains("Setup needs attention")
    );
    click(&mut rt, &toggle);
    assert!(rt.find("remote-checkout-parked").is_none());
    click(&mut rt, &toggle);
    reveal(&mut rt, "remote-checkout-parked");
    let (x, y) = rt.find("remote-checkout-parked").unwrap();
    rt.pointer_move(x, y);
    advance(&mut rt, 0.1);
    click(&mut rt, "remove-remote-parked");
    assert_eq!(
        live(&rt.state).panels.overlay,
        Some(Screen::RemoveVm),
        "{:?}",
        element(&rt, "remove-remote-parked")
    );
    click(&mut rt, "cancel-remove-vm");
    assert!(rt.find("remote-checkout-parked").is_some());
    secondary(&mut rt, "remote-checkout-parked");
    choose_context(&mut rt, "Remove from project…");
    click(&mut rt, "confirm-remove-vm");
    assert!(rt.find("remote-checkout-parked").is_none());
    assert_eq!(
        live(&rt.state).app.remote.saved.selected.as_deref(),
        Some(id)
    );
    click(&mut rt, "remove-failed-checkout");
    click(&mut rt, "confirm-remove-vm");
    assert!(rt.find(&toggle).is_none());
    assert_eq!(live(&rt.state).panels.screen, Screen::Home);
    assert!(
        live_mut(&mut rt.state)
            .app
            .commands()
            .iter()
            .all(|(_, op)| !matches!(
                op,
                Command::Remote(Operation::Power(..) | Operation::Create(..))
            ))
    );
}

#[test]
fn denied_boat_deletion_keeps_the_draft_and_background_success_stays_on_home() {
    let mut rt = design_runtime("", (800., 620.));
    let id = "checkout-example";
    rt.update(|s, _| crate::remote::scene(s, "remote-working"));
    advance(&mut rt, 0.2);
    fill(&mut rt, "remote-draft-checkout-example", "Unsent VM work");
    click(&mut rt, "remove-remote-checkout-example");
    click(&mut rt, "confirm-remove-vm");
    let request = |rt: &mut Runtime<Workspace>| {
        live_mut(&mut rt.state)
            .app
            .commands()
            .into_iter()
            .find_map(|(r, c)| match c {
                Command::Remote(Operation::Delete(key)) => Some((r, key)),
                _ => None,
            })
            .unwrap()
    };
    let (r, target) = request(&mut rt);
    assert!(!element(&rt, "confirm-remove-vm").focusable);
    rt.update(|s, _| live_mut(s).apply(Event::Completed(r, Err("Boat refused deletion".into()))));
    advance(&mut rt, 0.2);
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref() == Some("Boat refused deletion") && e.visible[3] > 0.)
    );
    assert_eq!(
        live(&rt.state).app.remote.saved.checkouts[id].thread.draft,
        "Unsent VM work"
    );
    click(&mut rt, "confirm-remove-vm");
    let (r, retry) = request(&mut rt);
    assert_eq!(retry, target);
    click(&mut rt, "cancel-remove-vm");
    click(&mut rt, "nav-home");
    rt.update(|s, _| {
        live_mut(s).apply(Event::Completed(
            r,
            Ok(Reply::Remote(Outcome::Deleted(target))),
        ))
    });
    advance(&mut rt, 0.2);
    assert_eq!(live(&rt.state).panels.screen, Screen::Home);
    assert!(!live(&rt.state).app.remote.saved.checkouts.contains_key(id));
}
