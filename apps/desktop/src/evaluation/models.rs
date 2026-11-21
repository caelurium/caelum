use crate::{Workspace, panels, settings};
use caelum_core::application::Provider;
use serde_json::json;

pub(super) fn scene(state: &mut Workspace, name: &str) {
    let l = state.live.as_mut().unwrap();
    l.app.sessions[0].scope.provider = Provider::OpenCode;
    l.app.sessions[0].options.model = Some("openai/gpt-5.4".into());
    let scope = l.app.sessions[0].scope.clone();

    let mut models = Vec::new();
    for (id, name, context) in [
        ("gpt-5.4", "GPT-5.4", 1_050_000),
        ("gpt-5.4-mini", "GPT-5.4 Mini", 400_000),
        ("gpt-5.4-nano", "GPT-5.4 Nano", 400_000),
        ("gpt-5.3-codex", "GPT-5.3 Codex", 400_000),
        ("gpt-5.2", "GPT-5.2", 400_000),
        ("gpt-5.2-codex", "GPT-5.2 Codex", 400_000),
        ("gpt-5.1", "GPT-5.1", 400_000),
        ("gpt-5.1-codex", "GPT-5.1 Codex", 400_000),
        ("gpt-5-mini", "GPT-5 Mini", 400_000),
        ("gpt-5-nano", "GPT-5 Nano", 400_000),
        ("gpt-4.1", "GPT-4.1", 1_000_000),
        ("o3", "o3", 200_000),
        ("o4-mini", "o4 Mini", 200_000),
    ] {
        models.push(json!({"id":format!("openai/{id}"),"name":name,"provider":{"id":"openai","name":"OpenAI"},"contextWindow":context,"efforts":[]}));
    }
    for (id, name, context) in [
        ("kimi-k2.5", "Kimi K2.5", 262_144),
        ("kimi-k2-thinking", "Kimi K2 Thinking", 262_144),
        ("kimi-k2", "Kimi K2", 131_072),
        ("minimax-m2.5", "MiniMax M2.5", 204_800),
        ("minimax-m2.1", "MiniMax M2.1", 204_800),
        ("minimax-m2", "MiniMax M2", 204_800),
        ("glm-5", "GLM-5", 204_800),
        ("glm-4.7", "GLM-4.7", 204_800),
        ("glm-4.6", "GLM-4.6", 204_800),
    ] {
        models.push(json!({"id":format!("opencode-go/{id}"),"name":name,"provider":{"id":"opencode-go","name":"OpenCode Go"},"contextWindow":context,"efforts":[]}));
    }
    l.app.choices.insert(
        scope.clone(),
        serde_json::from_value(
            json!({"models":models,"agents":[],"defaultModel":"openai/gpt-5.4"}),
        )
        .unwrap(),
    );
    l.app.preferences.favorite_models.insert(
        scope.provider.as_str().into(),
        vec!["openai/gpt-5.4".into(), "opencode-go/kimi-k2.5".into()],
    );
    if name == "models-settings" {
        l.panels.settings_page = settings::Page::Providers;
        l.panels.settings_provider = Provider::OpenCode;
        panels::show(state, panels::Screen::Settings);
    }
    crate::models::open(
        state,
        (name == "models-settings").then_some(Provider::OpenCode),
    );
    if name == "models-empty" {
        state
            .live
            .as_mut()
            .unwrap()
            .models
            .query
            .replace_text("no matching models");
    }
}
