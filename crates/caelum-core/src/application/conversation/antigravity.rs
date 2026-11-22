use serde_json::Value;

use super::{Conversation, Entry, EntryKind};

pub(super) fn history_entries(data: &[Value]) -> Vec<Entry> {
    data.iter()
        .filter_map(|item| {
            let id = item["id"].as_str()?.to_owned();
            match item["type"].as_str()? {
                "user" => {
                    let text = item["text"].as_str().unwrap_or_default().to_owned();
                    Some(Entry::new(id, EntryKind::User, text, true))
                }
                "assistant" => {
                    let text = item["text"].as_str().unwrap_or_default().to_owned();
                    Some(Entry::new(id, EntryKind::Assistant, text, true))
                }
                "reasoning" => {
                    let text = item["text"].as_str().unwrap_or_default().to_owned();
                    Some(Entry::new(id, EntryKind::Reasoning, text, true))
                }
                "tool" => {
                    let status = item["status"].as_str().unwrap_or("completed").to_owned();
                    let complete =
                        !matches!(status.as_str(), "pending" | "in_progress" | "running");
                    let mut entry = Entry::new(id, EntryKind::Tool, String::new(), complete);
                    entry.name = item["name"].as_str().unwrap_or("tool").to_owned();
                    entry.input = item["input"].as_str().unwrap_or_default().to_owned();
                    entry.output = item["output"].as_str().unwrap_or_default().to_owned();
                    entry.set_progress(complete, status);
                    Some(entry)
                }
                _ => None,
            }
        })
        .collect()
}

pub(super) fn apply_history_usage(conversation: &mut Conversation, data: &[Value]) {
    if let Some(usage) = data.iter().rev().find(|item| item["type"] == "usage") {
        if let Some(used) = usage["used"].as_u64() {
            conversation.used_tokens = Some(used);
        }
        if let Some(size) = usage["size"].as_u64() {
            conversation.context_window = Some(size);
        }
    }
}

pub(super) fn apply_event(conversation: &mut Conversation, method: &str, data: &Value) {
    match method {
        "antigravity/turnStarted" => {
            conversation.provider_started(data);
            conversation.set_working(true);
        }
        "antigravity/turnCompleted" => {
            if let Some(error) = data["error"].as_str().filter(|e| !e.is_empty()) {
                let id = format!("acp-error-{}", conversation.entries.len() + 1);
                let mut entry = Entry::new(id, EntryKind::Status, error.to_owned(), true);
                entry.error = error.to_owned();
                entry.set_progress(true, "failed");
                conversation.put(entry);
            }
            if let Some(ended_at) = data["_created"].as_u64() {
                conversation.settle_at(ended_at);
            } else {
                conversation.settle();
            }
        }
        "user_message_chunk" => {
            let text = crate::antigravity::protocol::chunk_text(data);
            if text.is_empty() {
                return;
            }
            let id = active_stream_id(conversation, EntryKind::User, "acp-live-user");
            conversation.delta(id, EntryKind::User, &text, false);
        }
        "agent_message_chunk" => {
            let text = crate::antigravity::protocol::chunk_text(data);
            if text.is_empty() {
                return;
            }
            let id = active_stream_id(conversation, EntryKind::Assistant, "acp-live-assistant");
            conversation.delta(id, EntryKind::Assistant, &text, false);
        }
        "agent_thought_chunk" => {
            let text = crate::antigravity::protocol::chunk_text(data);
            if text.is_empty() {
                return;
            }
            let id = active_stream_id(conversation, EntryKind::Reasoning, "acp-live-reasoning");
            conversation.delta(id, EntryKind::Reasoning, &text, false);
        }
        "tool_call" | "tool_call_update" => {
            let Some(tool_id) = data["toolCallId"].as_str().filter(|s| !s.is_empty()) else {
                return;
            };
            let id = format!("acp-tool-{tool_id}");
            let existing = conversation.entries.iter().find(|e| e.id == id);
            let status = data["status"]
                .as_str()
                .map(str::to_owned)
                .or_else(|| existing.map(|e| e.status().to_owned()))
                .unwrap_or_else(|| "running".into());
            let complete = !matches!(status.as_str(), "pending" | "in_progress" | "running");
            let mut entry = Entry::new(id, EntryKind::Tool, String::new(), complete);
            let name = crate::antigravity::protocol::tool_name(data);
            entry.name = if name != "tool" {
                name
            } else {
                existing.map(|e| e.name.clone()).unwrap_or(name)
            };
            let input = crate::antigravity::protocol::tool_input(data);
            entry.input = if !input.is_empty() {
                input
            } else {
                existing.map(|e| e.input.clone()).unwrap_or_default()
            };
            let output = crate::antigravity::protocol::tool_output(data);
            entry.output = if !output.is_empty() {
                output
            } else {
                existing.map(|e| e.output.clone()).unwrap_or_default()
            };
            entry.set_progress(complete, status);
            conversation.set_working(true);
            conversation.put(entry);
        }
        "usage_update" => {
            if let Some(used) = data["used"].as_u64() {
                conversation.used_tokens = Some(used);
            }
            if let Some(size) = data["size"].as_u64() {
                conversation.context_window = Some(size);
            }
        }
        _ => {}
    }
}

fn active_stream_id(conversation: &Conversation, kind: EntryKind, prefix: &str) -> String {
    if let Some(last) = conversation.entries.last() {
        if last.kind() == kind && !last.complete() {
            return last.id.clone();
        }
    }
    format!("{prefix}-{}", conversation.entries.len() + 1)
}
