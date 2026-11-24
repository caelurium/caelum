use super::{
    answered, append,
    wire::{reply, Wire},
    Turn,
};
use serde_json::json;
use std::path::Path;

pub(super) async fn ask(
    w: &mut Wire<'_>,
    scratch: &Path,
    parent: &str,
    turn: &Turn,
    publish: &mut impl FnMut(&str),
) -> Result<String, String> {
    w.request(1,"initialize",json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"caelum-side","version":env!("CARGO_PKG_VERSION")}})).await?;
    let s = w
        .request(2, "session/new", json!({"cwd":scratch,"mcpServers":[]}))
        .await?;
    let id = s["sessionId"]
        .as_str()
        .filter(|s| !s.is_empty() && *s != parent)
        .ok_or("Antigravity returned an unexpected side session.")?;
    if !s["modes"]["availableModes"]
        .as_array()
        .is_some_and(|v| v.iter().any(|m| m["id"] == "plan"))
    {
        return Err("This Antigravity version does not advertise read-only plan mode.".into());
    }
    if let Some(model) = &turn.model {
        if s["configOptions"].as_array().is_some_and(|opts| {
            opts.iter().any(|opt| {
                opt["id"] == "model"
                    && opt["options"].as_array().is_some_and(|models| {
                        models.iter().any(|m| m["value"].as_str() == Some(model))
                    })
            })
        }) {
            w.request(
                6,
                "session/set_config_option",
                json!({"sessionId":id,"configId":"model","value":model}),
            )
            .await?;
        }
    }
    if s["modes"]["currentModeId"] != "plan" {
        w.request(
            3,
            "session/set_mode",
            json!({"sessionId":id,"modeId":"plan"}),
        )
        .await?;
    }
    w.send(json!({"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"sessionId":id,"prompt":[{"type":"text","text":turn.text}]}})).await?;
    let mut text = String::new();
    loop {
        let m = w.next().await?;
        if let Some(r) = reply(&m, 4)? {
            if r["stopReason"] != "end_turn" {
                return Err("Antigravity did not finish the side question.".into());
            }
            let _ = w.request(5, "session/close", json!({"sessionId":id})).await;
            return answered(text);
        }
        let p = &m["params"];
        if m["method"] != "session/update" || p["sessionId"] != id {
            continue;
        }
        let u = &p["update"];
        if u["sessionUpdate"] == "current_mode_update" && u["currentModeId"] != "plan" {
            return Err("Antigravity left plan mode.".into());
        }
        if u["sessionUpdate"] == "agent_message_chunk" && u["content"]["type"] == "text" {
            append(&mut text, u["content"]["text"].as_str().unwrap_or(""))?;
            publish(&text)
        }
    }
}
