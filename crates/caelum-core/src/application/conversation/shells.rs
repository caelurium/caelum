//! Native shell records and tool-launched background completion messages.
use serde_json::{json, Value};

use super::{display, Entry, EntryKind};

fn synthetic_id(data: &Value) -> Option<&str> {
    if data["metadata"]["source"] != "shell" {
        return None;
    }
    ["shellID", "jobID"]
        .into_iter()
        .find_map(|key| data["metadata"][key].as_str().filter(|id| !id.is_empty()))
}

pub(in crate::application) fn completed_id<'a>(method: &str, data: &'a Value) -> Option<&'a str> {
    match method {
        "session.shell.ended" => data["shell"]["id"].as_str().filter(|id| !id.is_empty()),
        "session.synthetic" if data["metadata"]["state"] != "running" => synthetic_id(data),
        _ => None,
    }
}

pub(super) fn synthetic(data: &Value) -> Option<Entry> {
    let id = synthetic_id(data)?;
    let command = data["description"].as_str().unwrap_or_default();
    let state = data["metadata"]["state"].as_str().unwrap_or("completed");
    let text = data["text"].as_str().unwrap_or_default();
    // Use the structured command rather than scanning for '>' inside a quoted
    // attribute: commands can contain redirections, quotes and heredocs.
    let header = format!("<shell id=\"{id}\" state=\"{state}\" command=\"{command}\">");
    let output = text
        .strip_prefix(&header)
        .and_then(|body| body.strip_suffix("</shell>"))
        .map(|body| {
            let body = body.strip_prefix('\n').unwrap_or(body);
            body.strip_suffix('\n').unwrap_or(body)
        })
        .unwrap_or(text);
    entry(
        &json!({
            "id": id, "command": command, "status": state,
            "exit": data["metadata"]["exit"], "metadata": data["metadata"],
            "time": {"completed": data["_created"].as_u64().or(data["time"]["created"].as_u64())}
        }),
        &Value::String(output.into()),
    )
}

pub(super) fn history(message: &Value) -> Option<Entry> {
    entry(
        &json!({
            "id": message["shellID"].as_str().or(message["id"].as_str()),
            "command": message["command"], "status": message["status"],
            "exit": message["exit"], "metadata": message["metadata"],
            "time": {"started": message["time"]["created"], "completed": message["time"]["completed"]}
        }),
        &message["output"],
    )
}

pub(super) fn entry(shell: &Value, output: &Value) -> Option<Entry> {
    let id = shell["id"].as_str()?;
    let mut entry = Entry::new(
        format!("opencode-shell-{id}"),
        EntryKind::Tool,
        String::new(),
        shell["status"] != "running",
    );
    entry.name = "Background shell".into();
    entry.background = true;
    entry.started_at = shell["time"]["started"]
        .as_u64()
        .or(shell["started"].as_u64())
        .or(shell["startedAt"].as_u64());
    entry.metadata = shell["metadata"]
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into();
    for key in ["exit", "signal", "cwd"] {
        if !shell[key].is_null() {
            entry.metadata[key] = shell[key].clone();
        }
    }
    if let Some(ended) = shell["time"]["completed"]
        .as_u64()
        .or(shell["ended"].as_u64())
    {
        entry.metadata["ended"] = ended.into();
    }
    entry.input = display(&shell["command"]);
    entry.output = display(if output.is_object() {
        &output["output"]
    } else {
        output
    });
    entry.status = if shell["exit"].as_i64().is_some_and(|code| code != 0) {
        format!("failed (exit {})", shell["exit"])
    } else {
        match shell["status"].as_str().unwrap_or("completed") {
            "exited" => "completed",
            "timeout" => "timed_out",
            "killed" => "interrupted",
            status => status,
        }
        .into()
    };
    Some(entry)
}
