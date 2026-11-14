use crate::Workspace;
use serde_json::json;

pub(super) fn scene(state: &mut Workspace, name: &str) {
    let l = state.live.as_mut().unwrap();
    l.app.sessions[0].options.model = Some("provider/model-000".into());
    let scope = l.app.sessions[0].scope.clone();
    l.app.choices.insert(scope, serde_json::from_value(json!({"models": (0..160).map(|i| json!({"id":format!("provider/model-{i:03}"),"name":format!("Model {i:03}"),"contextWindow":200000,"efforts":[]})).collect::<Vec<_>>(),"agents":[],"defaultModel":"provider/model-000"})).unwrap());
    crate::models::open(state, None);
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
