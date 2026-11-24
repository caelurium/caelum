use super::*;
use caelum_core::remote::{Page, Run, RunStatus, TurnEnd};

fn reasoning_model() -> Runtime<Workspace> {
    let mut rt = fresh();
    click(&mut rt, "vm-provider-trigger");
    click(&mut rt, "option0");
    assert!(rt.find("vm-effort-trigger").is_none()); // the account's model is unknown
    click(&mut rt, "vm-model-trigger");
    let req = requested(&mut rt);
    complete(&mut rt, req, Ok(Outcome::Models(catalogue())));
    click(&mut rt, "vm-model-item-codex-only");
    rt
}

fn choose_level(rt: &mut Runtime<Workspace>, label: &str) {
    click(rt, "vm-effort-trigger");
    let key = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "option" && e.label.as_deref() == Some(label))
        .unwrap()
        .key
        .unwrap();
    click(rt, &key);
}

fn send(
    rt: &mut Runtime<Workspace>,
    text: &str,
    effort: Option<&str>,
    conversation: Option<&str>,
) -> u64 {
    let app = &mut live_mut(&mut rt.state).app;
    assert_eq!(app.remote.saved.checkouts[VM].thread.draft, text);
    assert!(
        app.commands().is_empty(),
        "editing effort must not start a turn"
    );
    click(rt, "remote-send");
    let cmds = live_mut(&mut rt.state).app.commands();
    let [(r, Command::Remote(Operation::Prompt(t, p)))] = cmds.as_slice() else {
        panic!("{cmds:?}")
    };
    assert_eq!(t.machine.id, "vm-example");
    assert_eq!(p.provider, VmProvider::Codex);
    assert_eq!(p.model.as_deref(), Some("codex-only"));
    assert_eq!(p.effort.as_deref(), effort);
    assert_eq!(p.conversation.as_deref(), conversation);
    assert_eq!(p.text, text);
    *r
}

#[test]
fn editing_effort_before_and_between_turns_only_applies_on_send() {
    let mut rt = reasoning_model();
    let owner = live(&rt.state).app.remote.saved.checkouts[VM]
        .thread
        .editor_key(VM)
        .to_owned();
    choose_level(&mut rt, "Low");
    choose_level(&mut rt, "Extra high");
    let req = send(&mut rt, DRAFT, Some("xhigh"), None);
    assert!(!element(&rt, "vm-effort-trigger").focusable);
    assert!(!live_mut(&mut rt.state).app.set_vm_effort(
        VM,
        &owner,
        VmProvider::Codex,
        "codex-only",
        Some("low")
    ));
    complete(
        &mut rt,
        req,
        Ok(Outcome::Accepted(Run {
            provider: VmProvider::Codex,
            conversation: "new-chat".into(),
            id: "r".into(),
            prompt: "p".into(),
        })),
    );
    let cmds = live_mut(&mut rt.state).app.commands();
    let [(r, Command::Remote(Operation::Follow(_, run, _)))] = cmds.as_slice() else {
        panic!("{cmds:?}")
    };
    assert_eq!(run.conversation, "new-chat");
    complete(
        &mut rt,
        *r,
        Ok(Outcome::Page(Page {
            lines: vec![],
            cursor: Some("page".into()),
            more: false,
            status: RunStatus::Ended(TurnEnd::Finished),
        })),
    );
    fill(
        &mut rt,
        &format!("remote-draft-{owner}"),
        "A quick follow-up",
    );
    // “None” is a real level; it is not the implicit Boat default.
    choose_level(&mut rt, "None");
    send(&mut rt, "A quick follow-up", Some("none"), Some("new-chat"));
}

#[test]
fn incompatible_choices_reset_and_a_saved_effort_can_be_cleared_offline() {
    let mut rt = reasoning_model();
    let owner = live(&rt.state).app.remote.saved.checkouts[VM]
        .thread
        .editor_key(VM)
        .to_owned();
    choose_level(&mut rt, "High");
    click(&mut rt, "vm-model-trigger");
    click(&mut rt, "vm-model-item-codex-basic");
    assert!(rt.find("vm-effort-trigger").is_none());
    let app = &mut live_mut(&mut rt.state).app;
    assert!(app.remote.saved.checkouts[VM].thread.effort.is_none());
    assert!(!app.set_vm_effort(VM, &owner, VmProvider::Codex, "codex-only", Some("high")));
    assert!(!app.set_vm_effort(VM, &owner, VmProvider::Codex, "codex-basic", Some("high")));
    click(&mut rt, "vm-model-trigger");
    click(&mut rt, "vm-model-item-codex-only");
    choose_level(&mut rt, "High");
    // Reopening a saved chat starts with an unloaded public catalogue.
    rt.update(|s, _| live_mut(s).app.remote.models = ModelCatalogue::Unloaded);
    click(&mut rt, "vm-effort-trigger");
    let req = requested(&mut rt);
    complete(&mut rt, req, Err("Offline".into()));
    assert_eq!(
        live(&rt.state).app.remote.saved.checkouts[VM]
            .thread
            .effort
            .as_deref(),
        Some("high")
    );
    assert!(rt.find("option1").is_none());
    click(&mut rt, "option0"); // Boat default still works
    send(&mut rt, DRAFT, None, None);
}

#[test]
fn a_catalogue_refresh_retires_only_levels_boat_removed() {
    let mut rt = reasoning_model();
    choose_level(&mut rt, "Extra high");
    let parked = live(&rt.state).app.remote.saved.checkouts[VM]
        .thread
        .editor_key(VM)
        .to_owned();
    click(&mut rt, "vm-threads");
    click(&mut rt, "new-vm-session");
    click(&mut rt, "vm-model-trigger");
    click(&mut rt, "vm-model-item-codex-only");
    choose_level(&mut rt, "Low");

    let mut revised = catalogue();
    let models = std::sync::Arc::make_mut(revised.get_mut(&VmProvider::Codex).unwrap());
    models[0].efforts.retain(|e| e.id != "xhigh");
    live_mut(&mut rt.state).app.refresh_vm_models();
    let req = requested(&mut rt);
    complete(&mut rt, req, Ok(Outcome::Models(revised)));
    assert_eq!(
        live(&rt.state).app.remote.saved.checkouts[VM]
            .thread
            .effort
            .as_deref(),
        Some("low")
    );
    click(&mut rt, "vm-threads");
    click(&mut rt, &format!("vm-thread-{parked}"));
    let thread = &live(&rt.state).app.remote.saved.checkouts[VM].thread;
    assert_eq!(thread.draft, DRAFT);
    assert_eq!(thread.model.as_deref(), Some("codex-only"));
    assert!(thread.effort.is_none());
    send(&mut rt, DRAFT, None, None);
}
