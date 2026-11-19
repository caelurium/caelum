use super::*;
use caelum_core::{application::Command, remote::Operation};

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
