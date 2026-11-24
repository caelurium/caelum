use super::{
    answered, append,
    wire::{reply, Wire},
    Turn,
};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::path::Path;

pub(super) async fn ask(
    w: &mut Wire<'_>,
    root: &Path,
    parent: &str,
    turn: &Turn,
    publish: &mut impl FnMut(&str),
) -> Result<String, String> {
    w.request(1,"initialize",json!({"clientInfo":{"name":"caelum-side","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}})).await?;
    w.send(json!({"method":"initialized","params":{}})).await?;
    let start=w.request(2,"thread/start",json!({"cwd":root,"model":turn.model,"ephemeral":true,"sandbox":"read-only","approvalPolicy":"never","serviceName":"caelum-side","dynamicTools":[],"config":{"mcp_servers":{},"features.apps":false,"web_search":"disabled"}})).await?;
    let id = start["thread"]["id"]
        .as_str()
        .filter(|id| {
            !id.is_empty() && *id != parent && start["thread"]["cwd"].as_str() == root.to_str()
        })
        .ok_or("Codex returned an unexpected side thread.")?;
    w.send(json!({"id":3,"method":"turn/start","params":{"threadId":id,"input":[{"type":"text","text":turn.text}],"approvalPolicy":"never","sandboxPolicy":{"type":"readOnly","networkAccess":false}}})).await?;
    let mut text = String::new();
    let mut complete = String::new();
    let mut early = VecDeque::new();
    let mut bytes = 0;
    let admitted = loop {
        let m = w.next().await?;
        if let Some(r) = reply(&m, 3)? {
            break r["turn"]["id"]
                .as_str()
                .ok_or("Missing side turn ID.")?
                .to_owned();
        }
        if m["params"]["threadId"] == id
            && matches!(
                m["method"].as_str(),
                Some("item/agentMessage/delta" | "item/completed" | "turn/completed")
            )
        {
            bytes += m.to_string().len();
            if bytes > super::LIMIT * 2 || early.len() >= 128 {
                return Err("Too many side updates before turn admission.".into());
            }
            early.push_back(m);
        }
    };
    loop {
        let msg = match early.pop_front() {
            Some(m) => m,
            None => w.next().await?,
        };
        let p = &msg["params"];
        if p["threadId"] == id
            && p["turnId"]
                .as_str()
                .or_else(|| p.pointer("/turn/id").and_then(Value::as_str))
                == Some(&admitted)
        {
            match msg["method"].as_str() {
                Some("item/agentMessage/delta") => {
                    append(&mut text, p["delta"].as_str().unwrap_or(""))?;
                    publish(&text)
                }
                Some("item/completed") if p["item"]["type"] == "agentMessage" => {
                    if !complete.is_empty() {
                        append(&mut complete, "\n\n")?
                    }
                    append(&mut complete, p["item"]["text"].as_str().unwrap_or(""))?;
                }
                Some("turn/completed") => {
                    if p["turn"]["status"] != "completed" {
                        return Err("Codex did not complete the side question.".into());
                    }
                    return answered(if complete.is_empty() { text } else { complete });
                }
                _ => {}
            }
        }
    }
}
