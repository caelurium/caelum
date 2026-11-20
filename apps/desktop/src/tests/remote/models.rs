mod effort;

use super::*;
use crate::live::{live, live_mut};
use caelum_core::remote::{ModelCatalogue, ModelLists};
use caelum_protocol::{ProviderChoice, ProviderModelChoice};

const VM: &str = "checkout-example";
const DRAFT: &str = "First message, still unsent.";

fn catalogue() -> ModelLists {
    let model = |id: &str, name: &str| ProviderModelChoice {
        id: id.into(),
        name: name.into(),
        provider: None,
        context_window: None,
        efforts: vec![],
        default_effort: None,
    };
    let mut open = vec![model("route:a", "Example quick")];
    open.extend((0..70).map(|i| model(&format!("other:{i}"), "Other model")));
    open.push(model("route:b", "Example detailed"));
    [
        (Provider::OpenCode, open.into()),
        (
            Provider::Codex,
            [
                ProviderModelChoice {
                    efforts: ["none", "low", "high", "xhigh"]
                        .map(|id| ProviderChoice {
                            id: id.into(),
                            name: id.into(),
                        })
                        .into(),
                    default_effort: Some("low".into()),
                    ..model("codex-only", "Example Codex")
                },
                model("codex-basic", "Basic model"),
            ]
            .into(),
        ),
    ]
    .into()
}

fn fresh() -> Runtime<Workspace> {
    let mut rt = design_runtime("", (800., 620.));
    rt.update(|s, _| crate::remote::scene(s, "remote-machine"));
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Escape, Mods::default());
    click(&mut rt, "vm-threads");
    click(&mut rt, "new-vm-session");
    let key = live(&rt.state).app.remote.saved.checkouts[VM]
        .thread
        .editor_key(VM);
    let field = format!("remote-draft-{key}");
    fill(&mut rt, &field, DRAFT);
    assert!(
        live_mut(&mut rt.state)
            .app
            .commands()
            .iter()
            .all(|(_, c)| !matches!(c, Command::Remote(_)))
    );
    rt
}

fn requested(rt: &mut Runtime<Workspace>) -> u64 {
    let cmds = live_mut(&mut rt.state).app.commands();
    let [(r, Command::Remote(Operation::Models))] = cmds.as_slice() else {
        panic!("{cmds:?}")
    };
    *r
}

fn complete(rt: &mut Runtime<Workspace>, r: u64, result: Result<Outcome, String>) {
    rt.update(|s, _| live_mut(s).apply(Event::Completed(r, result.map(Reply::Remote))));
    advance(rt, 0.2);
}

#[test]
fn choosing_a_then_b_only_submits_the_last_model_on_the_first_send() {
    for use_default in [false, true] {
        let mut rt = fresh();
        click(&mut rt, "vm-model-trigger");
        let req = requested(&mut rt);
        complete(&mut rt, req, Ok(Outcome::Models(catalogue())));
        assert!(rt.find("vm-model-item-codex-only").is_none());
        assert!(rt.find("vm-model-item-route:b").is_none());
        click(&mut rt, "vm-model-item-route:a");
        assert_eq!(
            live(&rt.state).app.remote.saved.checkouts[VM]
                .thread
                .model
                .as_deref(),
            Some("route:a")
        );
        click(&mut rt, "vm-model-trigger");
        fill(&mut rt, "vm-model-search", "DETAILED");
        assert!(rt.find("vm-model-item-route:a").is_none());
        key(&mut rt, Key::Enter, Mods::default());
        advance(&mut rt, 0.1);
        assert!(rt.find("vm-model-menu").is_none());
        if use_default {
            click(&mut rt, "vm-model-trigger");
            click(&mut rt, "vm-model-refresh");
            let r = requested(&mut rt);
            complete(&mut rt, r, Err("Offline".into()));
            click(&mut rt, "vm-model-default");
        }
        let expected = (!use_default).then_some("route:b");
        let app = &mut live_mut(&mut rt.state).app;
        assert!(app.commands().is_empty());
        let t = &app.remote.saved.checkouts[VM].thread;
        assert!(t.conversation.is_none() && t.lines.is_empty());
        assert_eq!(t.draft, DRAFT);
        assert_eq!(t.model.as_deref(), expected);
        click(&mut rt, "remote-send");
        let cmds = live_mut(&mut rt.state).app.commands();
        let [(_, Command::Remote(Operation::Prompt(target, prompt)))] = cmds.as_slice() else {
            panic!("{cmds:?}")
        };
        assert_eq!(target.machine.id, "vm-example");
        assert_eq!(prompt.provider, Provider::OpenCode);
        assert_eq!(prompt.model.as_deref(), expected);
        assert!(prompt.effort.is_none());
        assert!(prompt.conversation.is_none());
        assert_eq!(prompt.text, DRAFT);
        assert!(!element(&rt, "vm-model-trigger").focusable);
    }
}

#[test]
fn a_late_catalogue_does_not_change_a_draft_and_parked_models_stay_with_their_thread() {
    let mut rt = fresh();
    let owner = live(&rt.state).app.remote.saved.checkouts[VM]
        .thread
        .editor_key(VM)
        .to_owned();
    click(&mut rt, "vm-model-trigger");
    let first = requested(&mut rt);
    complete(&mut rt, first, Err("Offline".into()));
    assert!(element(&rt, "vm-model-default").focusable);
    click(&mut rt, "vm-model-refresh");
    let retry = requested(&mut rt);
    key(&mut rt, Key::Escape, Mods::default());
    click(&mut rt, "vm-provider-trigger");
    click(&mut rt, "option0"); // Codex
    click(&mut rt, "vm-model-trigger"); // reuse the one public request
    assert!(live_mut(&mut rt.state).app.commands().is_empty());
    complete(&mut rt, retry, Ok(Outcome::Models(catalogue())));
    assert!(rt.find("vm-model-item-route:b").is_none());
    let app = &mut live_mut(&mut rt.state).app;
    assert!(matches!(app.remote.models, ModelCatalogue::Ready(_)));
    assert!(!app.set_vm_model(VM, &owner, Provider::OpenCode, Some("route:b")));
    assert!(!app.set_vm_model(VM, &owner, Provider::Codex, Some("route:b")));
    click(&mut rt, "vm-model-item-codex-only");
    click(&mut rt, "vm-effort-trigger");
    click(&mut rt, "option3");
    assert_eq!(
        live(&rt.state).app.remote.saved.checkouts[VM].thread.draft,
        DRAFT
    );
    // An unsent session keeps its choices even if it has no text.
    fill(&mut rt, &format!("remote-draft-{owner}"), "");
    click(&mut rt, "vm-threads");
    click(&mut rt, "new-vm-session");
    let app = &mut live_mut(&mut rt.state).app;
    assert!(app.remote.saved.checkouts[VM].thread.model.is_none());
    assert!(!app.set_vm_model(VM, &owner, Provider::Codex, None));
    assert!(!app.set_vm_effort(VM, &owner, Provider::Codex, "codex-only", None));
    click(&mut rt, "vm-threads");
    click(&mut rt, &format!("vm-thread-{owner}"));
    let t = &live(&rt.state).app.remote.saved.checkouts[VM].thread;
    assert_eq!(
        (t.draft.as_str(), t.model.as_deref()),
        ("", Some("codex-only"))
    );
    assert_eq!(t.effort.as_deref(), Some("high"));
    click(&mut rt, "vm-provider-trigger");
    click(&mut rt, "option1");
    let thread = &live(&rt.state).app.remote.saved.checkouts[VM].thread;
    assert!(thread.model.is_none() && thread.effort.is_none());
    assert!(live_mut(&mut rt.state).app.commands().is_empty());
}
