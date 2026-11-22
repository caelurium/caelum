use super::excerpt::message_excerpt;
use super::tool_hint::{executable, tool_action, tool_detail};
use crate::application::{subagents, Entry, EntryKind, Provider, Session, Workspace};
use crate::remote::{Speaker, Thread, ToolState, Turn};
use serde_json::Value;

/// A turn's status, its latest public update and its start time in Unix milliseconds, when known.
#[derive(Debug, PartialEq, Eq)]
pub struct TurnPreview {
    pub label: &'static str,
    pub text: Option<String>,
    pub started_at: Option<u64>,
}

fn live_tool(entry: &Entry, root: &str) -> Option<String> {
    if entry.complete() || entry.name.trim().is_empty() {
        return None;
    }
    let input: Option<Value> = (entry.input.len() <= 8192)
        .then(|| serde_json::from_str(&entry.input).ok())
        .flatten();
    if let Some(description) = input
        .as_ref()
        .and_then(|v| v["description"].as_str())
        .and_then(message_excerpt)
    {
        return Some(description);
    }
    if entry.name == "subagent" {
        return input
            .as_ref()
            .and_then(|v| v["agent"].as_str())
            .and_then(message_excerpt);
    }
    let detail = input
        .as_ref()
        .and_then(|v| tool_detail(&entry.name, v, Some(root)))
        .or_else(|| {
            matches!(entry.name.as_str(), "commandExecution" | "Background shell")
                .then(|| executable(&entry.input).map(str::to_owned))
                .flatten()
        });
    Some(tool_action(&entry.name, detail.as_deref()))
}

impl Workspace {
    pub fn turn_preview(&self, session: &Session) -> TurnPreview {
        let c = &session.conversation;
        let root = self.checkout_directory(&session.scope.project);
        let mut preview = TurnPreview {
            label: if session.stopping() {
                "Stopping"
            } else if c.retrying() {
                "Retrying"
            } else if session.submitting() {
                "Sending"
            } else {
                "Running"
            },
            text: None,
            started_at: c.turn_started_at(),
        };
        let mut prompt = None;
        if c.working() {
            for entry in c.turn_entries() {
                if entry.kind() == EntryKind::User {
                    if c.is_turn_update(entry) {
                        prompt = message_excerpt(&entry.text);
                    }
                    break;
                }
                if !c.is_turn_update(entry) {
                    continue;
                }
                let text = match entry.kind() {
                    EntryKind::Assistant => message_excerpt(&entry.text),
                    EntryKind::Tool => live_tool(entry, root),
                    _ => None,
                };
                if text.is_some() {
                    preview.text = text;
                    return preview;
                }
            }
        }
        if let Some(task) = (session.scope.provider == Provider::OpenCode)
            .then(|| subagents::running_entries(&c.entries).next_back())
            .flatten()
        {
            preview.text = live_tool(task, root);
            preview.started_at = preview.started_at.or(task.started_at);
        } else if let Some(shell) = session.shells.last() {
            preview.text = Some(tool_action("shell", executable(&shell.command)));
            preview.started_at = preview.started_at.or(Some(shell.started_at));
        }
        preview.text = preview.text.or(prompt);
        preview
    }
}

impl TurnPreview {
    pub fn managed(thread: &Thread) -> Option<Self> {
        let label = match thread.turn {
            Turn::Idle => return None,
            Turn::Submitting => "Sending",
            Turn::Queued(_) => "Queued",
            Turn::Following(_) => "Running",
            Turn::Finishing(_) => "Finishing",
            Turn::Stopping(_) => "Stopping",
        };
        let mut text = None;
        for line in thread.lines.iter().rev() {
            match line.speaker {
                Speaker::Outcome(_) => break,
                Speaker::You => {
                    text = message_excerpt(&line.text);
                    break;
                }
                _ if matches!(thread.turn, Turn::Queued(_) | Turn::Submitting) => continue,
                Speaker::Tool(ToolState::Running) if !line.text.is_empty() => {
                    text = Some(tool_action(&line.text, line.tool_detail.as_deref()));
                }
                speaker if speaker.is_assistant() => text = message_excerpt(&line.text),
                _ => {}
            }
            if text.is_some() {
                break;
            }
        }
        Some(Self {
            label,
            text,
            started_at: thread.last_activity,
        })
    }
}
