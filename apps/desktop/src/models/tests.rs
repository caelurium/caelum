use super::*;
use brand::Brand;
use catalog::Group;
use serde_json::json;

fn catalogue() -> Workspace {
    let mut s = Workspace::live(None);
    let app = &mut live_mut(&mut s).app;
    let scope = Scope {
        project: "/models".into(),
        provider: Provider::OpenCode,
    };
    app.new_session(scope.clone());
    app.choices.insert(scope, serde_json::from_value(json!({
        "models": [
            {"id":"opencode-go/gpt-5", "name":"GPT-5", "provider":{"id":"opencode-go","name":"OpenCode Go"}, "contextWindow":400000, "efforts":[]},
            {"id":"openai/gpt-5", "name":"GPT-5", "provider":{"id":"openai","name":"OpenAI"}, "contextWindow":400000, "efforts":[]},
            {"id":"opencode-go/kimi-k2.5", "name":"Kimi K2.5", "provider":{"id":"opencode-go","name":"OpenCode Go"}, "efforts":[]},
            {"id":"gateway/anthropic/claude-sonnet", "name":"Claude Sonnet", "provider":{"id":"gateway","name":"Gateway"}, "efforts":[]},
            {"id":"local/custom", "name":"Custom model", "provider":{"id":"local","name":"Local runtime"}, "efforts":[]}
        ], "agents":[], "defaultModel":"openai/gpt-5"
    })).unwrap());
    open(&mut s, None);
    s
}

#[test]
fn company_groups_keep_gateway_identity_and_unknown_catalogues() {
    let s = catalogue();
    let items = matches(&s);
    let gpt: Vec<_> = items.iter().filter(|m| m.name == "GPT-5").collect();
    assert_eq!(gpt.len(), 2);
    assert!(
        gpt.iter()
            .all(|m| m.group == Group::Company("OpenAI".into()))
    );
    assert_ne!(gpt[0].id, gpt[1].id);
    assert_ne!(gpt[0].connection_id, gpt[1].connection_id);
    assert_eq!(
        items
            .iter()
            .find(|m| m.name == "Claude Sonnet")
            .unwrap()
            .brand,
        Some(Brand::Anthropic)
    );
    let custom = items.iter().find(|m| m.id == "local/custom").unwrap();
    assert_eq!(custom.brand, None);
    assert_eq!(custom.group, Group::Company("Local runtime".into()));
}

#[test]
fn filters_combine_company_connection_and_tokenized_search() {
    let mut s = catalogue();
    live_mut(&mut s)
        .models
        .query
        .replace_text("  OPENAI  400k gpt ");
    assert_eq!(
        matches(&s).len(),
        2,
        "company search includes models served by gateways"
    );
    filter(
        &mut s,
        Filter::Connection("opencode-go".into()),
        &mut Tx::default(),
    );
    assert_eq!(
        matches(&s)
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["opencode-go/gpt-5"]
    );
    live_mut(&mut s).models.query.replace_text("moonshot go");
    assert_eq!(matches(&s)[0].id, "opencode-go/kimi-k2.5");
    filter(&mut s, Filter::Favorites, &mut Tx::default());
    assert!(matches(&s).is_empty());
}

#[test]
fn favorites_are_scoped_and_missing_models_remain_removable() {
    let mut s = catalogue();
    live_mut(&mut s)
        .app
        .preferences
        .favorite_models
        .insert("codex".into(), vec!["local/custom".into()]);
    favorite(&mut s, Provider::OpenCode, "opencode-go/kimi-k2.5");
    favorite(&mut s, Provider::OpenCode, "missing/model");
    live_mut(&mut s).app.sessions[0].options.model = Some("missing/model".into());
    let items = matches(&s);
    assert_eq!(items[0].group, Group::Default);
    assert_eq!(items[1].id, "opencode-go/kimi-k2.5");
    assert_eq!(items[1].group, Group::Favorites);
    assert_eq!(items.last().unwrap().id, "missing/model");
    assert!(!items.last().unwrap().available);
    assert!(
        !items
            .iter()
            .find(|m| m.id == "local/custom")
            .unwrap()
            .favorite
    );
    filter(&mut s, Filter::Favorites, &mut Tx::default());
    assert_eq!(matches(&s).len(), 2);
    favorite(&mut s, Provider::OpenCode, "missing/model");
    assert_eq!(matches(&s).len(), 1);
    filter(&mut s, Filter::All, &mut Tx::default());
    assert_eq!(
        matches(&s)
            .iter()
            .filter(|m| m.id == "missing/model")
            .count(),
        1
    );
}

#[test]
fn families_use_their_company_even_through_other_connections() {
    for (id, expected) in [
        ("qwen3-coder", Brand::Alibaba),
        ("claude-sonnet", Brand::Anthropic),
        ("deepseek-r1", Brand::DeepSeek),
        ("gemini-pro", Brand::Google),
        ("llama-4", Brand::Meta),
        ("MiniMax-M2", Brand::MiniMax),
        ("devstral", Brand::Mistral),
        ("kimi-k2", Brand::Moonshot),
        ("o3", Brand::OpenAi),
        ("grok-4", Brand::Xai),
        ("glm-5", Brand::Zai),
    ] {
        assert_eq!(
            Brand::model(&format!("gateway/{id}"), "Model", "gateway"),
            Some(expected),
            "{id}"
        );
    }
    assert_eq!(
        Brand::model("custom", "Custom", "codex"),
        Some(Brand::OpenAi)
    );
    assert_eq!(Brand::model("custom", "Custom", "local"), None);
}
