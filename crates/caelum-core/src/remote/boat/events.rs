use crate::remote::{Line, Run, Speaker, ToolState};
use boat_sdk::agents::models::SandboxEvent;

/// Boat sends snapshots: the tool result is paired with its `use`, not another
/// chat message. Keep call identity and outcome without copying raw inputs or output.
pub(super) fn lines(events: Vec<SandboxEvent>, run: &Run, repository: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for e in events {
        // Older response events can carry only taskId (the Boat prompt ID).
        // Never render an unowned event or an earlier prompt from this conversation.
        let task = e.task_id.as_deref();
        let conversation = e.conversation_id.as_deref();
        if e.kind != "response"
            || task.is_some_and(|id| id != run.prompt)
            || conversation.is_some_and(|id| id != run.conversation)
            || (conversation.is_none() && task.is_none())
        {
            continue;
        }
        let Some(data) = e.data else { continue };
        if data.get("is_reverted").and_then(|v| v.as_bool()) == Some(true) {
            continue;
        }
        let event = e.id.as_deref().filter(|id| super::harness::valid_id(id));
        if let Some(text) = data
            .get("content")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            lines.push(Line {
                key: event.map(|id| format!("{}:response:{id}", run.id)),
                partial: data
                    .get("is_streaming")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                ..Line::new(
                    Speaker::OpenCode,
                    &text[..text.floor_char_boundary(
                        crate::remote::Thread::MAX_MESSAGE_BYTES.min(text.len()),
                    )],
                )
            });
        }
        for (i, tool) in data
            .get("tools")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .enumerate()
        {
            let call = tool.get("use").unwrap_or(tool);
            let Some(name) = call
                .get("name")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
            else {
                continue;
            };
            let key = call
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|id| super::harness::valid_id(id))
                .map(|id| format!("{}:tool:{id}", run.id))
                .or_else(|| event.map(|id| format!("{}:event-tool:{id}:{i}", run.id)));
            let state = match tool.get("result").filter(|r| !r.is_null()) {
                Some(r) if r.get("is_error").and_then(|v| v.as_bool()) == Some(true) => {
                    ToolState::Failed
                }
                Some(_) => ToolState::Complete,
                None if tool.get("use").is_some() => ToolState::Running,
                None => ToolState::Reported,
            };
            lines.push(Line {
                key,
                tool_detail: call.get("input").and_then(|v| detail(name, v, repository)),
                ..Line::new(
                    Speaker::Tool(state),
                    name.chars()
                        .filter(|c| !c.is_control())
                        .take(80)
                        .collect::<String>(),
                )
            });
        }
    }
    lines
}

fn detail(name: &str, input: &serde_json::Value, repository: &str) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "bash" | "shell" => {
            let word = input.get("command")?.as_str()?.split_whitespace().next()?;
            if !word
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))
            {
                return None;
            }
            let program = word.rsplit('/').next()?;
            (!program.is_empty() && program.len() <= 40).then(|| program.to_owned())
        }
        "read" | "edit" | "write" => {
            let path = ["filePath", "file_path", "path"]
                .iter()
                .find_map(|p| input.get(p)?.as_str())?;
            let path = if path.starts_with('/') {
                let repo = repository.rsplit('/').next()?;
                path.strip_prefix("/home/user/")?
                    .strip_prefix(repo)?
                    .strip_prefix('/')?
            } else {
                path.trim_start_matches("./")
            };
            (!path.is_empty()
                && !path.starts_with(['~', '/'])
                && path.len() <= 160
                && !path.chars().any(char::is_control)
                && !path.split('/').any(|p| p == ".."))
            .then(|| path.to_owned())
        }
        _ => None,
    }
}
