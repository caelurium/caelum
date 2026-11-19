use super::*;
use caelum_core::{
    application::{Command, Event, Reply},
    remote::{Inventory, Operation, Outcome, Source},
};

#[test]
fn settings_create_vm_opens_the_form_and_only_provisions_on_submit() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-settings"));
    advance(&mut rt, 0.2);
    let entry = element(&rt, "open-create-vm");
    assert_eq!(entry.label.as_deref(), Some("Create VM…"));
    assert!(entry.visible[3] >= 28.);
    click(&mut rt, "open-create-vm");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        assert_eq!(l.panels.overlay, Some(crate::panels::Screen::CreateVm));
        assert!(l.app.remote.saved.checkouts.is_empty());
        let (id, project) = l
            .app
            .commands()
            .into_iter()
            .find_map(|(id, command)| match command {
                Command::Remote(Operation::Discover(project)) => Some((id, project)),
                _ => None,
            })
            .unwrap();
        assert_eq!(Some(&project), l.app.active_project.as_ref());
        l.apply(Event::Completed(
            id,
            Ok(Reply::Remote(Outcome::Source(Source {
                account: "example-account".into(),
                project,
                repository: "example/project".into(),
                base: "main".into(),
                environment: None,
            }))),
        ));
    });
    advance(&mut rt, 0.2);
    assert!(!element(&rt, "create-remote").focusable);
    fill(&mut rt, "remote-branch", "feat/remote-ui");
    let submit = element(&rt, "create-remote");
    assert_eq!(submit.label.as_deref(), Some("Create VM"));
    assert!(submit.focusable && submit.visible[3] >= 28.);
    click(&mut rt, "create-remote");
    let l = rt.state.live.as_mut().unwrap();
    assert_eq!(l.panels.screen, crate::panels::Screen::Remote);
    assert!(l.panels.overlay.is_none());
    let creates: Vec<_> = l
        .app
        .commands()
        .into_iter()
        .filter_map(|(_, c)| match c {
            Command::Remote(Operation::Create(s, p)) => Some((s, p)),
            _ => None,
        })
        .collect();
    assert!(
        matches!(creates.as_slice(),[(s,p)] if s.repository=="example/project" && s.base=="main" && p.branch=="feat/remote-ui")
    );
}

#[test]
fn launcher_create_vm_can_connect_and_choose_a_folder_from_an_empty_workspace() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.leave_conversation();
        l.app.active_project = None;
        l.app.projects.clear();
        crate::panels::open_launcher(s);
    });
    advance(&mut rt, 0.2);
    click(&mut rt, "open-create-vm");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let (id, _) = l
            .app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Remote(Operation::Inventory)))
            .unwrap();
        l.apply(Event::Completed(id, Err("No saved Boat key".into())));
    });
    advance(&mut rt, 0.2);
    fill(&mut rt, "boat-api-key", "fixture-key");
    click(&mut rt, "connect-boat");
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let (id, _) = l
            .app
            .commands()
            .into_iter()
            .find(|(_, c)| matches!(c, Command::Remote(Operation::Connect(_))))
            .unwrap();
        l.apply(Event::Completed(
            id,
            Ok(Reply::Remote(Outcome::Inventory(Inventory {
                account: "example-account".into(),
                machines: vec![],
            }))),
        ));
    });
    advance(&mut rt, 0.2);
    assert!(element(&rt, "browse-vm-project").visible[3] >= 28.);
    assert!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .remote
            .saved
            .checkouts
            .is_empty()
    );
}

#[test]
fn remote_composer_and_workspace_menu_target_the_vm_when_the_local_checkout_was_open() {
    let mut rt = design_runtime("", (1280., 900.));
    rt.update(|s, _| crate::remote::scene(s, "remote-machine"));
    advance(&mut rt, 0.3);
    let badge = element(&rt, "remote-badge");
    assert!(
        badge.visible[2] >= 70. && badge.visible[3] >= 26.,
        "{badge:?}"
    );
    assert!(rt.find("remote-pin-stats").is_some());
    click(&mut rt, "remote-badge");
    fill(
        &mut rt,
        "remote-draft-checkout-example",
        "Review the remote branch",
    );
    key(&mut rt, Key::Enter, Mods::default());
    let app = &mut rt.state.live.as_mut().unwrap().app;
    assert!(app.commands().iter().any(|(_,op)|matches!(op,Command::Remote(Operation::Prompt(target,Some(c),text)) if target.machine.id=="vm-example" && c=="thread-example" && text=="Review the remote branch")));
    assert!(app.active_project.is_none());
    choose_workspace_action(&mut rt, "View remote changes");
    assert!(rt.state.live.as_mut().unwrap().app.commands().iter().any(|(_,op)|matches!(op,Command::Remote(Operation::Git(target,_)) if target.machine.id=="vm-example")));
}

#[test]
fn compact_remote_review_keeps_both_actions_and_the_machine_badge_visible() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-changes"));
    advance(&mut rt, 0.4);
    for key in ["remote-commit", "remote-push", "remote-badge"] {
        let e = element(&rt, key);
        assert!(
            e.rect
                .iter()
                .zip(e.visible)
                .all(|(a, b)| (*a - b).abs() < 1.),
            "{key}: {e:?}"
        );
    }
    assert!(rt.find("remote-pinned-stats").is_none());
}
