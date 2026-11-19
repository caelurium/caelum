use super::{
    api,
    events::{event_id, response},
    harness::valid_id,
    Boat,
};
use crate::remote::{MachineId, ToolCall, ToolOutput, ToolSource, ToolState};
use boat_sdk::agents::{EventOrder, EventQuery};
use serde_json::Value;

pub(super) const MAX_CURSOR: usize = 2048;
const MAX_OUTPUT: usize = 16 * 1024;
const MAX_COMMAND: usize = 2048;

pub(super) struct Call<'a> {
    pub name: &'a str,
    pub id: Option<&'a str>,
    pub input: Option<&'a Value>,
    result: Option<&'a Value>,
    pub state: ToolState,
}

impl<'a> Call<'a> {
    pub fn read(tool: &'a Value) -> Option<Self> {
        let usage = tool.get("use").unwrap_or(tool);
        let result = tool.get("result").filter(|r| !r.is_null());
        let name = usage.get("name").and_then(Value::as_str).unwrap_or("");
        let id = usage.get("id").and_then(Value::as_str);
        let result_id = result.and_then(|r| r["tool_use_id"].as_str());
        if id.zip(result_id).is_some_and(|(a, b)| a != b) {
            return None;
        }
        let id = id.or(result_id).filter(|id| event_id(id));
        if name.trim().is_empty() && (result.is_none() || id.is_none()) {
            return None;
        }
        let state = match result {
            Some(r) if r["is_error"] == true => ToolState::Failed,
            Some(_) => ToolState::Complete,
            None if tool.get("use").is_some() => ToolState::Running,
            None => ToolState::Reported,
        };
        Some(Self {
            name,
            id,
            input: usage.get("input"),
            result,
            state,
        })
    }
    pub fn identity(&self, slot: usize) -> Option<ToolCall> {
        if let Some(id) = self.id {
            return Some(ToolCall::Id(id.into()));
        }
        // For a call without an ID, require the same full name at the same slot.
        (self.name.len() <= 80 && !self.name.chars().any(char::is_control))
            .then(|| ToolCall::Slot(slot as u16, self.name.into()))
    }

    fn matches(&self, slot: usize, key: &ToolCall) -> bool {
        match key {
            ToolCall::Id(id) => self.id == Some(id),
            ToolCall::Slot(index, name) => {
                self.id.is_none() && slot == usize::from(*index) && name == self.name
            }
        }
    }

    fn preview(&self) -> ToolOutput {
        let mut clipped = false;
        let mut command = String::new();
        let shell = matches!(
            self.name.to_ascii_lowercase().as_str(),
            "bash" | "shell" | "commandexecution" | "exec_command"
        );
        if shell {
            if let Some(s) = self
                .input
                .and_then(|i| i.get("command").or_else(|| i.get("cmd")))
                .and_then(Value::as_str)
            {
                clipped |= append(&mut command, s, MAX_COMMAND);
            }
        }
        let mut output = String::new();
        let mut state = self.state;
        if let Some(content) = self.result.and_then(|r| r.get("content")) {
            if let Some(text) = content.as_str() {
                if let Some((code, stdout, stderr)) = shell.then(|| command_result(text)).flatten()
                {
                    clipped |= append(&mut output, &stdout, MAX_OUTPUT);
                    if !stdout.is_empty() && !stdout.ends_with('\n') && !stderr.is_empty() {
                        clipped |= append(&mut output, "\n", MAX_OUTPUT);
                    }
                    clipped |= append(&mut output, &stderr, MAX_OUTPUT);
                    if code != 0 || output.is_empty() {
                        if !output.is_empty() && !output.ends_with('\n') {
                            clipped |= append(&mut output, "\n", MAX_OUTPUT);
                        }
                        clipped |= append(&mut output, &format!("Exit {code}"), MAX_OUTPUT);
                    }
                    if code != 0 {
                        state = ToolState::Failed;
                    }
                } else {
                    clipped |= append(&mut output, text, MAX_OUTPUT);
                }
            } else if let Some(parts) = content.as_array() {
                for part in parts
                    .iter()
                    .filter(|p| p["type"] == "text")
                    .filter_map(|p| p["text"].as_str())
                {
                    if !output.is_empty() {
                        clipped |= append(&mut output, "\n", MAX_OUTPUT);
                    }
                    if append(&mut output, part, MAX_OUTPUT) {
                        clipped = true;
                        break;
                    }
                }
            }
        }
        ToolOutput {
            command,
            output,
            clipped,
            state,
        }
    }
}

// Boat's documented Bash result may encode a command result as JSON text.
fn command_result(text: &str) -> Option<(i64, String, String)> {
    if text.len() > MAX_OUTPUT * 2 || !text.trim_start().starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(text).ok()?;
    let code = v["exitCode"].as_i64()?;
    // Other result formats (including payloads with additional display
    // fields) should remain readable rather than becoming an empty exit code.
    if v.get("stdout").is_none() && v.get("stderr").is_none() && v.as_object()?.len() != 1 {
        return None;
    }
    Some((
        code,
        v["stdout"].as_str().unwrap_or("").into(),
        v["stderr"].as_str().unwrap_or("").into(),
    ))
}

fn append(out: &mut String, text: &str, max: usize) -> bool {
    let end = text.floor_char_boundary(text.len().min(max.saturating_sub(out.len())));
    out.extend(
        text[..end]
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t')),
    );
    end < text.len()
}

impl Boat {
    pub(in crate::remote) async fn tool_output(
        &self,
        key: &MachineId,
        source: &ToolSource,
    ) -> Result<ToolOutput, String> {
        self.owns(key)?;
        if !valid_id(&source.conversation)
            || !valid_id(&source.prompt)
            || !event_id(&source.event)
            || source.before.as_ref().is_some_and(|s| s.len() > MAX_CURSOR)
        {
            return Err("Invalid saved Boat tool reference".into());
        }
        let page = self
            .client
            .events(
                &key.id,
                &EventQuery {
                    conversation: Some(source.conversation.clone()),
                    cursor: source.before.clone(),
                    sort: Some(EventOrder::Asc),
                    limit: Some(100),
                    ..Default::default()
                },
            )
            .await
            .map_err(api)?;
        if page.id != key.id {
            return Err("Boat returned a different machine".into());
        }
        page.events
            .iter()
            .filter(|e| e.id.as_deref() == Some(&source.event))
            .filter_map(|e| response(e, &source.conversation, &source.prompt))
            .filter_map(|d| d.get("tools")?.as_array())
            .flat_map(|ts| ts.iter().take(200).enumerate())
            .find_map(|(i, t)| {
                let c = Call::read(t)?;
                c.matches(i, &source.call).then_some(c)
            })
            .map(|c| c.preview())
            .ok_or_else(|| "Boat no longer lists this tool call at its saved event.".into())
    }
}
