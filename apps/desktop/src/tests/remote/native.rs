use super::*;
use crate::live::{live, live_mut};

#[test]
fn save_as_default_on_a_vm_keeps_its_model_effort_and_favorites_off_the_laptop_and_other_vms() {
    use caelum_core::application::Options;
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "native-connected"));
    advance(&mut rt, 0.2);
    let slot = live(&rt.state).app.selected.unwrap();
    let scope = live(&rt.state).app.sessions[slot].scope.clone();
    let options = Options {
        model: Some("on-vm".into()),
        effort: Some("high".into()),
        agent: Some("plan".into()),
    };
    rt.update(|s, _| {
        let app = &mut live_mut(s).app;
        let mut second = app.remote.saved.checkouts["checkout-example"].clone();
        if let caelum_core::remote::Phase::Ready { machine, .. } = &mut second.phase {
            machine.key.id = "vm-two".into();
        }
        app.remote
            .saved
            .checkouts
            .insert("other-checkout".into(), second);
        app.sessions[slot].options = options.clone();
        app.options_changed(slot);
        app.toggle_favorite_model(&scope, "on-vm");
    });
    let preferences = live(&rt.state).app.preferences.clone();
    click(&mut rt, "model-trigger");
    click(&mut rt, "save-provider-defaults");
    key(&mut rt, Key::Escape, Mods::default());
    click(&mut rt, "new-session");
    let app = &mut live_mut(&mut rt.state).app;
    let fresh = app.selected.unwrap();
    assert_ne!(fresh, slot);
    assert_eq!(app.sessions[fresh].options, options);
    assert_eq!(app.preferences.providers, preferences.providers);
    assert_eq!(
        app.preferences.default_provider,
        preferences.default_provider
    );
    let other = app
        .open_vm_session("other-checkout", Some(scope.provider))
        .unwrap();
    assert_eq!(app.sessions[other].options, Options::default());
    assert!(app.favorite_models(&app.sessions[other].scope).is_none());
    assert_eq!(app.favorite_models(&scope).unwrap(), ["on-vm"]);
    assert!(app.commands().iter().all(|(_, c)| !matches!(
        c,
        Command::Send(..) | Command::Start(..) | Command::Enqueue(..)
    )));
}

#[test]
fn first_send_keeps_its_vm_and_options_when_the_user_switches_back_to_a_boat_chat() {
    use caelum_core::application::{SessionKey, Summary};
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-machine"));
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Escape, Mods::default());
    click(&mut rt, "new-session");
    let i = live(&rt.state).app.selected.unwrap();
    rt.update(|s, _| live_mut(s).app.set_effort(i, Some("high".into())));
    fill(
        &mut rt,
        &format!("live-draft-{i}"),
        "First explicit message",
    );
    assert!(
        live_mut(&mut rt.state)
            .app
            .commands()
            .iter()
            .all(|(_, c)| !matches!(
                c,
                Command::Start(..) | Command::Send(..) | Command::Remote(Operation::Prompt(..))
            ))
    );
    click(&mut rt, "send");
    let commands = live_mut(&mut rt.state).app.commands();
    let [(start, Command::Start(scope, options))] = commands.as_slice() else {
        panic!("{commands:?}")
    };
    assert_eq!(scope.project, "boat://checkout/checkout-example");
    assert_eq!(options.effort.as_deref(), Some("high"));
    let key = SessionKey {
        scope: scope.clone(),
        id: "ses_vm_first".into(),
    };
    let prior = "vm-session-checkout-example/checkout-example";
    reveal(&mut rt, prior);
    click(&mut rt, prior);
    rt.update(|s, _| {
        live_mut(s).apply(Event::Completed(
            *start,
            Ok(Reply::Session(Summary {
                key: key.clone(),
                title: "Native session".into(),
                updated_at: None,
                working: false,
                options: options.clone(),
            })),
        ))
    });
    let l = live_mut(&mut rt.state);
    assert_eq!(l.panels.screen, crate::panels::Screen::Remote);
    assert_eq!(l.app.sessions[i].key(), Some(key.clone()));
    let sends = l
        .app
        .commands()
        .into_iter()
        .filter(|(_, c)| {
            matches!(
                c,
                Command::Send(..) | Command::Start(..) | Command::Remote(Operation::Prompt(..))
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(sends.as_slice(), [(_, Command::Send(k,text,opts))] if k==&key && text=="First explicit message" && opts==options)
    );
}

#[test]
fn going_offline_preserves_the_editor_and_recovery_only_reads_the_native_session() {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "native-connected"));
    advance(&mut rt, 0.1);
    let i = live(&rt.state).app.selected.unwrap();
    let key = live(&rt.state).app.sessions[i].key().unwrap();
    assert_eq!(key.scope.project, "boat://checkout/checkout-example");
    fill(
        &mut rt,
        &format!("live-draft-{i}"),
        "keep this offline draft",
    );
    rt.update(|s, _| {
        let l = live_mut(s);
        let events = l.app.native_connection_lost(&key.scope.project);
        l.apply_events(events);
        let id = "checkout-example";
        l.native.fixture(
            id,
            l.app.remote.saved.checkouts[id].native_target().unwrap(),
            true,
        );
    });
    advance(&mut rt, 0.1);
    click(&mut rt, "send");
    assert!(rt.find("native-workspace-reconnect").is_some());
    click(&mut rt, &format!("live-draft-{i}"));
    super::key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.1);
    assert_eq!(live(&rt.state).drafts[i].text, "keep this offline draft");
    let app = &mut live_mut(&mut rt.state).app;
    app.native_connected(&key.scope.project);
    let requests = app.commands();
    assert!(
        requests
            .iter()
            .any(|(_, c)| matches!(c,Command::Resume(k) if k==&key))
    );
    assert!(!requests.iter().any(|(_, c)| matches!(
        c,
        Command::Send(..) | Command::Start(..) | Command::Enqueue(..) | Command::Remote(_)
    )));
    assert_eq!(app.sessions[i].key().as_ref(), Some(&key));
}
