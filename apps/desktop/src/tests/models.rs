use super::*;

fn grouped_runtime(size: (f32, f32)) -> Runtime<Workspace> {
    let mut rt = live_runtime_size(size);
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        let session = &mut app.sessions[1];
        session.options.model = Some("openai/gpt-shared".into());
        app.choices.insert(session.scope.clone(), serde_json::from_value(serde_json::json!({
            "models": [
                {"id":"openai/gpt-shared", "name":"GPT Shared", "provider":{"id":"openai","name":"OpenAI"}, "contextWindow":1000000, "efforts":[], "defaultEffort":"high"},
                {"id":"opencode-go/gpt-shared", "name":"GPT Shared", "provider":{"id":"opencode-go","name":"OpenCode Go"}, "contextWindow":1000000, "efforts":[], "defaultEffort":"medium"},
                {"id":"opencode-go/kimi-k2", "name":"Kimi K2", "provider":{"id":"opencode-go","name":"OpenCode Go"}, "contextWindow":262144, "efforts":[]},
                {"id":"opencode-go/glm-5", "name":"GLM-5", "provider":{"id":"opencode-go","name":"OpenCode Go"}, "contextWindow":204800, "efforts":[]}
            ], "agents":[], "defaultModel":"openai/gpt-shared"
        })).unwrap());
        app.select(1);
    });
    advance(&mut rt, 0.2);
    click(&mut rt, "model-trigger");
    rt
}

#[test]
fn company_search_and_connection_filters_keep_duplicate_model_identity() {
    let mut rt = grouped_runtime((800.0, 620.0));
    fill(&mut rt, "model-search", "openai shared 1m");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("2 models")
    );
    assert!(element(&rt, "model-group-OpenAI").visible[3] > 0.0);
    click(&mut rt, "model-filter-connection-opencode-go");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("1 model")
    );
    assert!(element(&rt, "model-search").focused);
    click(&mut rt, "model-favorite-0");
    assert_eq!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .preferences
            .favorite_models["opencode"],
        ["opencode-go/gpt-shared"]
    );
    click(&mut rt, "model-filter-favorites");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("1 model")
    );

    rt.update(|s, _| s.live.as_mut().unwrap().app.select(0));
    key(&mut rt, Key::Enter, Mods::default());
    let app = &rt.state.live.as_ref().unwrap().app;
    assert_eq!(
        app.sessions[1].options.model.as_deref(),
        Some("opencode-go/gpt-shared")
    );
    assert_eq!(app.sessions[1].options.effort.as_deref(), Some("medium"));
    assert!(app.sessions[0].options.model.is_none());

    rt.update(|s, _| s.live.as_mut().unwrap().app.select(1));
    advance(&mut rt, 0.2);
    click(&mut rt, "model-trigger");
    click(&mut rt, "model-filter-connection-openai");
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .options
            .model
            .as_deref(),
        Some("openai/gpt-shared")
    );
}

#[test]
fn filter_keyboard_focus_empty_results_and_unavailable_favorites_are_usable() {
    let mut rt = grouped_runtime((800.0, 620.0));
    key(&mut rt, Key::Tab, Mods::default());
    assert!(element(&rt, "model-filter-all").focused);
    key(&mut rt, Key::Tab, Mods::default());
    assert!(element(&rt, "model-filter-favorites").focused);
    key(&mut rt, Key::Text(" ".into()), Mods::default());
    advance(&mut rt, 0.2);
    assert!(
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.text.as_deref() == Some("No favourite models"))
    );
    key(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.live.as_ref().unwrap().models.is_open());
    click(&mut rt, "model-filter-all");
    fill(&mut rt, "model-search", "Moonshot Go");
    assert!(element(&rt, "model-group-Moonshot AI").visible[3] > 0.0);
    click(&mut rt, "model-filter-connection-openai");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("0 models")
    );
    click(&mut rt, "model-filter-all");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("1 model")
    );
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        app.preferences
            .favorite_models
            .insert("opencode".into(), vec!["missing/model".into()]);
        app.sessions[1].options.model = Some("missing/model".into());
    });
    fill(&mut rt, "model-search", "missing");
    assert!(element(&rt, "model-group-Unavailable").visible[3] > 0.0);
    assert!(
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.label.as_deref() == Some("missing/model") && !e.focusable)
    );
    key(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.live.as_ref().unwrap().models.is_open());
    click(&mut rt, "model-favorite-0");
    assert!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .preferences
            .favorite_models["opencode"]
            .is_empty()
    );
    click(&mut rt, "model-filter-favorites");
    assert_eq!(
        element(&rt, "model-result-count").text.as_deref(),
        Some("0 models")
    );
}

#[test]
fn grouped_settings_picker_fits_and_keeps_session_and_provider_defaults_separate() {
    for size in [(800.0, 620.0), (1280.0, 1000.0)] {
        for theme in ["dark", "light"] {
            let mut rt = grouped_runtime(size);
            key(&mut rt, Key::Escape, Mods::default());
            rt.update(|s, _| s.live.as_mut().unwrap().app.preferences.theme = theme.into());
            choose_workspace_action(&mut rt, "Settings…");
            click(&mut rt, "settings-providers");
            click(&mut rt, "settings-provider-opencode");
            click(&mut rt, "settings-model-trigger");
            advance(&mut rt, 0.3);
            let popup = element(&rt, "model-popover");
            assert!(
                popup
                    .rect
                    .iter()
                    .zip(popup.visible)
                    .all(|(a, b)| (*a - b).abs() < 1.0),
                "{size:?} {theme}: {popup:?}"
            );
            assert!(element(&rt, "model-scroll").rect[3] >= 186.0);
            click(&mut rt, "model-filter-connection-opencode-go");
            fill(&mut rt, "model-search", "shared");
            key(&mut rt, Key::Enter, Mods::default());
            advance(&mut rt, 0.2);
            let app = &rt.state.live.as_ref().unwrap().app;
            assert_eq!(
                app.preferences.providers["opencode"].model.as_deref(),
                Some("opencode-go/gpt-shared")
            );
            assert_eq!(
                app.sessions[1].options.model.as_deref(),
                Some("openai/gpt-shared")
            );
            assert!(element(&rt, "settings-model-trigger").focused);
            click(&mut rt, "settings-model-trigger");
            fill(&mut rt, "model-search", "provider default");
            key(&mut rt, Key::Enter, Mods::default());
            assert!(
                rt.state.live.as_ref().unwrap().app.preferences.providers["opencode"]
                    .model
                    .is_none()
            );
        }
    }
}

#[test]
fn selecting_a_session_loads_its_model_effort_and_agent_controls() {
    use caelum_core::application::{Command, Event, Provider, Reply, Scope};

    let mut rt = live_runtime();
    let mut background = 0;
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.projects.push(caelum_protocol::ProjectInfo {
            path: "/background".into(),
            name: "Background".into(),
        });
        let index = l.app.new_session(Scope {
            project: "/background".into(),
            provider: Provider::OpenCode,
        });
        background = index;
        let session = &mut l.app.sessions[index];
        session.remote_id = Some("restored-session".into());
        session.draft = "Keep this café draft".into();
        session.options.model = Some("local/model".into());
        session.options.effort = Some("low".into());
        session.options.agent = Some("build".into());
        l.sync_editors();
    });
    advance(&mut rt, 0.3);
    let commands = rt.state.live.as_mut().unwrap().app.commands();
    let requests: Vec<_> = commands
        .iter()
        .filter_map(|(id, command)| match command {
            Command::Choices(scope) if scope.project == "/background" => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(requests.len(), 1, "navigation must load the catalogue");

    rt.update(|s, _| {
        s.live.as_mut().unwrap().apply(Event::Completed(
            requests[0],
            Err("Provider unavailable".into()),
        ));
    });
    advance(&mut rt, 1.0);
    assert!(rt.state.live.as_mut().unwrap().app.commands().is_empty());

    rt.update(|s, _| s.live.as_mut().unwrap().app.select(0));
    advance(&mut rt, 0.1);
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(background));
    advance(&mut rt, 0.1);
    let retry = rt
        .state
        .live
        .as_mut()
        .unwrap()
        .app
        .commands()
        .into_iter()
        .find_map(|(id, command)| match command {
            Command::Choices(scope) if scope.project == "/background" => Some(id),
            _ => None,
        })
        .expect("revisiting the session must retry a missing catalogue");
    rt.update(|s, _| {
        s.live.as_mut().unwrap().apply(Event::Completed(
            retry,
            Ok(Reply::Choices(
                serde_json::from_value(serde_json::json!({
                    "models": [{
                        "id": "local/model", "name": "Local model",
                        "efforts": [{"id":"low","name":"Low"}, {"id":"high","name":"High"}]
                    }],
                    "agents": [{"id":"build","name":"Build"}, {"id":"plan","name":"Plan"}],
                    "defaultModel": "local/model"
                }))
                .unwrap(),
            )),
        ));
    });
    advance(&mut rt, 0.2);
    assert!(rt.find("model-trigger").is_some());
    assert!(rt.find("effort-trigger").is_some());
    assert!(rt.find("agent-trigger").is_some());
    click(&mut rt, "effort-trigger");
    key(&mut rt, Key::End, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    click(&mut rt, "agent-trigger");
    key(&mut rt, Key::End, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    let l = rt.state.live.as_ref().unwrap();
    assert_eq!(l.app.selected, Some(background));
    assert_eq!(l.drafts[background].text, "Keep this café draft");
    assert_eq!(
        l.app.sessions[background].options.effort.as_deref(),
        Some("high")
    );
    assert_eq!(
        l.app.sessions[background].options.agent.as_deref(),
        Some("plan")
    );

    rt.update(|s, _| s.live.as_mut().unwrap().app.select(0));
    advance(&mut rt, 0.1);
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(background));
    advance(&mut rt, 0.1);
    assert!(rt.state.live.as_mut().unwrap().app.commands().is_empty());
}

#[test]
fn native_shortcuts_and_large_model_catalogue_keep_the_captured_owner() {
    let mut rt = live_runtime_size((800.0, 620.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let scope = l.app.sessions[1].scope.clone();
        l.app.choices.insert(scope, serde_json::from_value(serde_json::json!({
            "models": (0..160).map(|i| serde_json::json!({"id":format!("model-{i:03}"),"name":format!("Model {i:03}"),"efforts":[],"defaultEffort":null})).collect::<Vec<_>>(),
            "agents":[{"id":"build","name":"Build"},{"id":"plan","name":"Plan"}],"defaultModel":"model-000"
        })).unwrap());
        l.app.sessions[1].options.model = Some("model-000".into());
        l.app.sessions[1].options.agent = Some("build".into());
        l.app.select(1);
    });
    advance(&mut rt, 0.3);
    fill(&mut rt, "live-draft-1", "Preserve café and the caret");
    key(
        &mut rt,
        Key::Tab,
        Mods {
            shift: true,
            ..Default::default()
        },
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .options
            .agent
            .as_deref(),
        Some("plan")
    );
    assert!(element(&rt, "live-draft-1").focused);
    click(&mut rt, "model-trigger");
    advance(&mut rt, 0.3);
    for _ in 0..105 {
        key(&mut rt, Key::Down, Mods::default());
    }
    advance(&mut rt, 0.2);
    assert!(element(&rt, "option106").visible[3] > 0.0);
    assert!(rt.elements().len() < 1000, "catalogue must be windowed");
    key(&mut rt, Key::Tab, Mods::default());
    assert!(!element(&rt, "model-search").focused);
    key(&mut rt, Key::Down, Mods::default());
    advance(&mut rt, 0.2);
    assert!(element(&rt, "model-search").focused);
    assert!(element(&rt, "option107").visible[3] > 0.0);
    fill(&mut rt, "model-search", "159");
    rt.update(|s, _| s.live.as_mut().unwrap().app.select(0));
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);
    let l = rt.state.live.as_ref().unwrap();
    assert_eq!(l.app.selected, Some(0));
    assert_eq!(
        l.app.sessions[1].options.model.as_deref(),
        Some("model-159")
    );
    assert_eq!(l.drafts[1].text, "Preserve café and the caret");
    assert_ne!(
        l.app.sessions[0].options.model.as_deref(),
        Some("model-159")
    );
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        l.app.sessions[0].remote_id = Some("thread".into());
        l.app.sessions[0].conversation.set_working(true);
    });
    key(
        &mut rt,
        Key::Command(','),
        Mods {
            cmd: true,
            ..Default::default()
        },
    );
    advance(&mut rt, 0.3);
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.3);
    assert!(!rt.state.live.as_ref().unwrap().app.sessions[0].stopping());
    key(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.live.as_ref().unwrap().app.sessions[0].stopping());
}

#[test]
fn model_popover_stays_anchored_and_returns_focus_without_cancelling_work() {
    let app = spec();
    let mut state = crate::evaluation::design();
    crate::evaluation::scene(&mut state, "models");
    let mut rt = Runtime::new(state, app.view, (800.0, 620.0), Env::new(true, 1.0));
    rt.poll = app.poll;
    rt.on_key = app.on_key;
    advance(&mut rt, 0.4);
    let popup = element(&rt, "model-popover");
    let trigger = element(&rt, "model-trigger");
    assert!(popup.rect[1] + popup.rect[3] <= trigger.rect[1]);
    assert!(
        popup
            .rect
            .into_iter()
            .zip(popup.visible)
            .all(|(a, b)| (a - b).abs() < 1.0)
    );
    assert!(
        !rt.elements()
            .iter()
            .any(|e| !e.exiting && e.key.as_deref() == Some("workspace-dialog"))
    );
    let defaults = element(&rt, "save-provider-defaults");
    assert!(defaults.rect[1] + defaults.rect[3] <= popup.rect[1] + popup.rect[3]);
    rt.update(|s, _| {
        s.live.as_mut().unwrap().app.sessions[0]
            .conversation
            .set_working(true);
    });
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.4);
    assert!(!rt.state.live.as_ref().unwrap().models.is_open());
    assert!(!rt.state.live.as_ref().unwrap().app.sessions[0].stopping());
    click(&mut rt, "model-trigger");
    rt.pointer_down(250.0, 80.0);
    rt.pointer_up(250.0, 80.0);
    advance(&mut rt, 0.4);
    assert!(!rt.state.live.as_ref().unwrap().models.is_open());
}

#[test]
fn models_slash_command_opens_the_picker_for_the_current_session() {
    let mut rt = live_runtime_size((800.0, 620.0));
    fill(&mut rt, "live-draft-0", "/models");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.3);

    let live = rt.state.live.as_ref().unwrap();
    assert!(live.models.is_open());
    assert!(live.app.sessions[0].draft.is_empty());
    assert!(!live.app.sessions[0].submitting());
    let commands = rt.state.live.as_mut().unwrap().app.commands();
    assert!(commands.iter().any(|(_, command)| {
        matches!(
            command,
            caelum_core::application::Command::Choices(scope)
                if scope.project == "/project"
                    && scope.provider == caelum_core::application::Provider::Codex
        )
    }));
    assert!(
        rt.elements()
            .iter()
            .any(|element| { !element.exiting && element.key.as_deref() == Some("model-popover") })
    );
}

#[test]
fn variants_and_effort_slash_commands_change_model_effort() {
    let mut rt = live_runtime_size((800.0, 620.0));
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        let scope = app.sessions[0].scope.clone();
        app.sessions[0].options.model = None;
        app.choices.insert(
            scope,
            serde_json::from_value(serde_json::json!({
                "models": [{
                    "id": "model-with-efforts",
                    "name": "Effort model",
                    "efforts": [
                        {"id":"medium","name":"Medium"},
                        {"id":"xhigh","name":"Extra high"},
                        {"id":"max","name":"Maximum"}
                    ],
                    "defaultEffort": "medium"
                }],
                "agents": [],
                "defaultModel": "model-with-efforts"
            }))
            .unwrap(),
        );
    });

    fill(&mut rt, "live-draft-0", "/variants");
    key(&mut rt, Key::Enter, Mods::default());
    advance(&mut rt, 0.2);
    assert!(rt.find("effort-content").is_some());
    key(&mut rt, Key::Escape, Mods::default());
    advance(&mut rt, 0.2);

    fill(&mut rt, "live-draft-0", "/effort xhigh");
    key(&mut rt, Key::Enter, Mods::default());
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[0]
            .options
            .effort
            .as_deref(),
        Some("xhigh")
    );

    fill(&mut rt, "live-draft-0", "/effort default");
    key(&mut rt, Key::Enter, Mods::default());
    assert!(
        rt.state.live.as_ref().unwrap().app.sessions[0]
            .options
            .effort
            .is_none()
    );
}

#[test]
fn opencode_effort_default_controls_reset_sent_and_queued_prompts() {
    use caelum_core::application::{Command, Provider};
    for command in [Some("/effort default"), Some("/effort model default"), None] {
        for working in [false, true] {
            let mut rt = live_runtime_size((800.0, 620.0));
            rt.update(|s, _| {
                let app = &mut s.live.as_mut().unwrap().app;
                let session = &mut app.sessions[0];
                session.scope.provider = Provider::OpenCode;
                session.remote_id = Some("ses_fake".into());
                session.options.model = Some("local/model/a".into());
                session.options.effort = Some("high".into());
                session.conversation.set_working(working);
                app.choices.insert(
                    session.scope.clone(),
                    serde_json::from_value(serde_json::json!({
                        "models": [{
                            "id": "local/model/a", "name": "Model A",
                            "efforts": [{"id": "high", "name": "High"}]
                        }],
                        "agents": [],
                        "defaultModel": "local/model/a"
                    }))
                    .unwrap(),
                );
                app.commands();
            });
            advance(&mut rt, 0.1);
            if let Some(command) = command {
                fill(&mut rt, "live-draft-0", command);
                key(&mut rt, Key::Enter, Mods::default());
            } else {
                click(&mut rt, "effort-trigger");
                click(&mut rt, "option0");
            }
            fill(
                &mut rt,
                "live-draft-0",
                "Continue with model default effort",
            );
            key(&mut rt, Key::Enter, Mods::default());
            let app = &mut rt.state.live.as_mut().unwrap().app;
            let commands = app.commands();
            let options = if working {
                assert!(!commands.iter().any(|(_, command)| matches!(
                    command,
                    Command::Send(..) | Command::Enqueue(..)
                )));
                assert_eq!(app.sessions[0].queue.len(), 1);
                &app.sessions[0].queue[0].options
            } else {
                commands
                    .iter()
                    .find_map(|(_, command)| match command {
                        Command::Send(_, _, options) => Some(options),
                        _ => None,
                    })
                    .expect("prompt must reach the provider")
            };
            assert_eq!(options.model.as_deref(), Some("local/model/a"));
            assert_eq!(options.effort.as_deref(), Some(""));
        }
    }
}

#[test]
fn settings_provider_and_composer_choices_apply_to_first_send() {
    use caelum_core::application::{Command, Provider, Scope};
    use caelum_protocol::{ProviderChoice, ProviderChoices, ProviderModelChoice};
    let mut rt = live_runtime();
    rt.update(|s, _| {
        let app = &mut s.live.as_mut().unwrap().app;
        app.choices.insert(
            Scope {
                provider: Provider::OpenCode,
                project: "/project".into(),
            },
            ProviderChoices {
                models: ["first", "second"]
                    .into_iter()
                    .map(|id| ProviderModelChoice {
                        id: id.into(),
                        name: id.into(),
                        provider: None,
                        context_window: None,
                        efforts: vec![ProviderChoice {
                            id: "low".into(),
                            name: "Low".into(),
                        }],
                        default_effort: Some("low".into()),
                    })
                    .collect(),
                agents: vec![ProviderChoice {
                    id: "build".into(),
                    name: "Build".into(),
                }],
                default_model: Some("first".into()),
            },
        );
    });
    fill(&mut rt, "live-draft-0", "Keep this Codex draft");
    choose_workspace_action(&mut rt, "Settings…");
    click(&mut rt, "settings-providers");
    click(&mut rt, "default-provider-opencode");
    key(&mut rt, Key::Escape, Mods::default());
    click(&mut rt, "new-session");
    let index = rt.state.live.as_ref().unwrap().app.selected.unwrap();
    let draft = format!("live-draft-{index}");
    assert_ne!(index, 0);
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[0].text,
        "Keep this Codex draft"
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[0]
            .scope
            .provider,
        Provider::Codex
    );
    fill(&mut rt, &draft, "Continue this draft");
    click(&mut rt, "provider-trigger");
    click(&mut rt, "option0");
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(index));
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[index].text,
        "Continue this draft"
    );
    assert!(element(&rt, &draft).focused);
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[index]
            .scope
            .provider,
        Provider::Codex
    );
    assert_eq!(
        rt.state.live.as_ref().unwrap().drafts[0].text,
        "Keep this Codex draft"
    );
    click(&mut rt, "provider-trigger");
    click(&mut rt, "option1");
    assert_eq!(rt.state.live.as_ref().unwrap().app.selected, Some(index));
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[index]
            .scope
            .provider,
        Provider::OpenCode
    );
    click(&mut rt, "model-trigger");
    fill(&mut rt, "model-search", "second");
    click(&mut rt, "model-favorite-0");
    assert!(element(&rt, "model-search").focused);
    assert_eq!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .preferences
            .favorite_models["opencode"],
        vec!["second"]
    );
    key(&mut rt, Key::Enter, Mods::default());
    assert!(element(&rt, &draft).focused);
    click(&mut rt, "model-trigger");
    fill(&mut rt, "model-search", "first");
    key(&mut rt, Key::Escape, Mods::default());
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[index]
            .options
            .model
            .as_deref(),
        Some("second")
    );
    click(&mut rt, "model-trigger");
    fill(&mut rt, "model-search", "second");
    click(&mut rt, "option0");
    click(&mut rt, "agent-trigger");
    key(&mut rt, Key::End, Mods::default());
    key(&mut rt, Key::Enter, Mods::default());
    click(&mut rt, &draft);
    rt.paste("Use my choices");
    key(&mut rt, Key::Enter, Mods::default());
    let (_, command) = rt
        .state
        .live
        .as_mut()
        .unwrap()
        .app
        .commands()
        .pop()
        .unwrap();
    assert!(
        matches!(&command, Command::Start(scope, options) if scope.provider == Provider::OpenCode
        && options.model.as_deref() == Some("second") && options.effort.as_deref() == Some("low") && options.agent.as_deref() == Some("build")),
        "{command:?}"
    );
    click(&mut rt, "model-trigger");
    fill(&mut rt, "model-search", "first");
    click(&mut rt, "option0");
    click(&mut rt, &draft);
    rt.paste("Queue with a different model");
    key(&mut rt, Key::Enter, Mods::default());
    let queued = &rt.state.live.as_ref().unwrap().app.sessions[index].queue;
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].options.model.as_deref(), Some("first"));
}

#[test]
fn shift_tab_cycles_agents_in_split_view_unfocused_and_home() {
    let mut rt = live_runtime_size((1280.0, 900.0));
    rt.update(|s, _| {
        let l = s.live.as_mut().unwrap();
        let opencode_scope = l.app.sessions[1].scope.clone();
        l.app.choices.insert(
            opencode_scope,
            serde_json::from_value(serde_json::json!({
                "models": [{"id":"m1","name":"Model 1","efforts":[],"defaultEffort":null}],
                "agents": [{"id":"build","name":"Build"},{"id":"plan","name":"Plan"}],
                "defaultModel": "m1"
            }))
            .unwrap(),
        );
        l.app.sessions[1].options.agent = Some("build".into());
        l.app.select(0);
        crate::split::open_session(s, 1, crate::split::SplitOrientation::Vertical);
    });
    advance(&mut rt, 0.2);

    let shift_tab = Mods {
        shift: true,
        ..Default::default()
    };
    // Secondary split pane cycles repeatedly without losing focus to the primary pane.
    key(&mut rt, Key::Tab, shift_tab);
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .options
            .agent
            .as_deref(),
        Some("plan")
    );
    assert!(element(&rt, "live-draft-1").focused);
    key(&mut rt, Key::Tab, shift_tab);
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .options
            .agent
            .as_deref(),
        Some("build")
    );
    assert!(element(&rt, "live-draft-1").focused);

    // Works even when the composer input is not focused.
    rt.update(|s, _| {
        for draft in &mut s.live.as_mut().unwrap().drafts {
            draft.focused = false;
        }
    });
    advance(&mut rt, 0.1);
    key(&mut rt, Key::Tab, shift_tab);
    assert_eq!(
        rt.state.live.as_ref().unwrap().app.sessions[1]
            .options
            .agent
            .as_deref(),
        Some("plan")
    );
    assert!(element(&rt, "live-draft-1").focused);

    // Home screen cycles agents for the active Home scope too.
    rt.update(|s, _| {
        crate::home::open(s);
        let l = s.live.as_mut().unwrap();
        l.app
            .home_set_provider(caelum_core::application::Provider::OpenCode);
        if let Some(scope) = l.app.home_scope() {
            l.app.choices.insert(
                scope,
                serde_json::from_value(serde_json::json!({
                    "models": [{"id":"m1","name":"Model 1","efforts":[],"defaultEffort":null}],
                    "agents": [{"id":"build","name":"Build"},{"id":"plan","name":"Plan"}],
                    "defaultModel": "m1"
                }))
                .unwrap(),
            );
        }
    });
    advance(&mut rt, 0.2);
    key(&mut rt, Key::Tab, shift_tab);
    assert_eq!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .home_options()
            .agent
            .as_deref(),
        Some("build")
    );
    key(&mut rt, Key::Tab, shift_tab);
    assert_eq!(
        rt.state
            .live
            .as_ref()
            .unwrap()
            .app
            .home_options()
            .agent
            .as_deref(),
        Some("plan")
    );
}
