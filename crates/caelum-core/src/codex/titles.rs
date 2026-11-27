use std::path::Path;

use caelum_protocol::{CodexEvent, ProviderChoices};
use serde_json::{json, Value};

use super::{transport::publish_event, CodexError, CodexProvider};

const MAX_TURNS: usize = 12;
const MAX_TURN_CHARS: usize = 400;
const MAX_EXCERPT_CHARS: usize = 3_600;
const MAX_TITLE_CHARS: usize = 80;

pub(crate) fn thread_title(name: Option<String>, preview: String) -> String {
    name.filter(|v| !v.is_empty()).unwrap_or(preview)
}

impl CodexProvider {
    /// Generate a concise title for a Codex thread from its recent conversation,
    /// persist it via `thread/name/set`, and emit `thread/name/updated`.
    /// Works for both local and VM Codex sessions via `side_command`.
    pub fn regenerate_title(
        &self,
        project_path: impl AsRef<Path>,
        thread_id: &str,
    ) -> Result<(), CodexError> {
        if thread_id.trim().is_empty() {
            return Err(CodexError::InvalidThreadId);
        }
        let project = self.project(project_path)?;
        let connection = self.connection(&project)?;
        connection.owned_thread(&project, thread_id, false)?;

        let response = connection.request(
            "thread/turns/list",
            json!({
                "threadId": thread_id,
                "limit": MAX_TURNS,
                "sortDirection": "desc",
                "itemsView": "full",
            }),
        )?;
        let data = response
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| CodexError::Protocol("thread/turns/list has no data array".into()))?;
        let excerpt = format_excerpt(data)
            .ok_or_else(|| CodexError::Protocol("No conversation messages to title".into()))?;

        let model = self
            .choices(project.path())
            .ok()
            .and_then(|choices| fast_model(&choices));
        let path_str = project.path().to_string_lossy().into_owned();
        let cmd = self
            .side_command(&path_str, thread_id)
            .map_err(CodexError::Transport)?;
        let prompt = format!(
            "Generate a concise, descriptive session title (3 to 7 words, no quotes, no trailing punctuation, no markdown, single line) that captures the current topic of this conversation. Reply with ONLY the title.\n\nConversation:\n{excerpt}"
        );
        let raw = crate::side_questions::run_codex_prompt(
            &self.inner.processes,
            cmd,
            project.path(),
            thread_id,
            prompt,
            model,
        )
        .map_err(CodexError::Protocol)?;
        let title = sanitize_title(&raw)
            .ok_or_else(|| CodexError::Protocol("Codex title response was empty".into()))?;

        connection.request(
            "thread/name/set",
            json!({ "threadId": thread_id, "name": title }),
        )?;
        publish_event(
            &self.inner.subscribers,
            CodexEvent {
                project_path: path_str,
                method: "thread/name/updated".into(),
                params: json!({
                    "threadId": thread_id,
                    "threadName": title,
                }),
                request_id: None,
            },
        );
        Ok(())
    }
}

fn format_excerpt(turns_desc: &[Value]) -> Option<String> {
    let mut messages = Vec::new();
    for turn in turns_desc.iter().rev() {
        let Some(items) = turn.get("items").and_then(Value::as_array) else {
            continue;
        };
        for item in items {
            let (label, text) = match item.get("type").and_then(Value::as_str) {
                Some("userMessage") => {
                    let raw = user_message_text(item);
                    let cleaned = crate::application::split_context(&raw).0.trim().to_owned();
                    ("User", cleaned)
                }
                Some("agentMessage") => {
                    let raw = item
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    ("Assistant", raw)
                }
                _ => continue,
            };
            if text.is_empty() {
                continue;
            }
            let capped: String = text.chars().take(MAX_TURN_CHARS).collect();
            messages.push(format!("{label}: {capped}"));
        }
    }
    let mut lines = Vec::new();
    let mut total = 0usize;
    for line in messages.into_iter().rev().take(MAX_TURNS) {
        let len = line.chars().count();
        if total > 0 && total + len + 1 > MAX_EXCERPT_CHARS {
            break;
        }
        total += len + 1;
        lines.push(line);
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

fn user_message_text(item: &Value) -> String {
    let Some(content) = item.get("content").and_then(Value::as_array) else {
        return item
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
    };
    content
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn fast_model(choices: &ProviderChoices) -> Option<String> {
    for needle in ["mini", "spark"] {
        if let Some(model) = choices
            .models
            .iter()
            .find(|m| m.id.to_ascii_lowercase().contains(needle))
        {
            return Some(model.id.clone());
        }
    }
    None
}

fn sanitize_title(raw: &str) -> Option<String> {
    let line = raw.lines().map(str::trim).find(|line| !line.is_empty())?;
    let stripped = line
        .trim_start_matches(['#', '-', '*', ' '])
        .trim_start_matches("Title:")
        .trim_start_matches("title:")
        .trim()
        .trim_matches(['"', '\'', '`'])
        .trim();
    let mut collapsed = String::new();
    let mut last_space = false;
    for ch in stripped.chars() {
        if ch.is_control() || ch.is_whitespace() {
            if !last_space && !collapsed.is_empty() {
                collapsed.push(' ');
                last_space = true;
            }
        } else {
            collapsed.push(ch);
            last_space = false;
        }
    }
    let cleaned = collapsed
        .trim()
        .trim_end_matches(['.', '!', '?', ':', ';'])
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    let truncated: String = cleaned.chars().take(MAX_TITLE_CHARS).collect();
    let final_title = truncated.trim();
    (!final_title.is_empty()).then(|| final_title.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caelum_protocol::ProviderModelChoice;

    #[test]
    fn formats_recent_user_and_assistant_messages_in_chronological_order() {
        let turns_desc = vec![
            json!({
                "id": "t2",
                "items": [
                    { "type": "userMessage", "content": [{ "type": "text", "text": "Fix the VM title bug" }] },
                    { "type": "commandExecution", "command": "cargo test" },
                    { "type": "agentMessage", "text": "I wired up thread/name/set." }
                ]
            }),
            json!({
                "id": "t1",
                "items": [
                    { "type": "userMessage", "content": [{ "type": "text", "text": "Check codex titles" }] }
                ]
            }),
        ];
        assert_eq!(
            format_excerpt(&turns_desc).as_deref(),
            Some("User: Check codex titles\nUser: Fix the VM title bug\nAssistant: I wired up thread/name/set.")
        );
    }

    #[test]
    fn prefers_mini_or_spark_model_when_available() {
        let choices = ProviderChoices {
            models: vec![
                ProviderModelChoice {
                    id: "gpt-5.4".into(),
                    name: "GPT-5.4".into(),
                    provider: None,
                    context_window: None,
                    efforts: vec![],
                    default_effort: None,
                },
                ProviderModelChoice {
                    id: "gpt-5.4-mini".into(),
                    name: "GPT-5.4 Mini".into(),
                    provider: None,
                    context_window: None,
                    efforts: vec![],
                    default_effort: None,
                },
            ],
            agents: vec![],
            default_model: Some("gpt-5.4".into()),
        };
        assert_eq!(fast_model(&choices).as_deref(), Some("gpt-5.4-mini"));
    }

    #[test]
    fn sanitizes_markdown_quotes_and_trailing_punctuation() {
        assert_eq!(
            sanitize_title("  # Title: \"Fix VM Codex Titles.\"\nSecond line").as_deref(),
            Some("Fix VM Codex Titles")
        );
        assert_eq!(sanitize_title("   "), None);
    }
}
