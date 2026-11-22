use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
};

use caelum_protocol::{
    AntigravitySession, OpenCodeCommandInfo, ProviderChoice, ProviderChoices, ProviderModelChoice,
    SessionHistoryPage,
};
use serde_json::{json, Value};

use super::SessionOptionState;

pub(super) fn parse_session_list(
    result: &Value,
    project_path: &Path,
    working: &HashSet<String>,
    options: &HashMap<String, SessionOptionState>,
) -> Vec<AntigravitySession> {
    let expected = project_path.to_string_lossy();
    let Some(items) = result["sessions"].as_array() else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item["sessionId"].as_str()?.trim();
            if id.is_empty() {
                return None;
            }
            if let Some(cwd) = item["cwd"].as_str() {
                if Path::new(cwd) != project_path && cwd != expected.as_ref() {
                    return None;
                }
            }
            let title = item["title"].as_str().unwrap_or_default().trim().to_owned();
            let updated_at = parse_timestamp_ms(&item["updatedAt"]);
            let opts = options.get(id);
            Some(AntigravitySession {
                id: id.to_owned(),
                title,
                updated_at,
                is_working: working.contains(id),
                model: opts.and_then(|o| o.model.clone()),
                effort: opts.and_then(|o| o.effort.clone()),
                agent: opts.and_then(|o| o.agent.clone()),
            })
        })
        .collect()
}

pub(super) fn parse_choices(result: &Value) -> ProviderChoices {
    let config_options = result["configOptions"].as_array();
    let mut effort_labels: BTreeMap<String, String> = BTreeMap::new();
    let mut global_efforts: Vec<ProviderChoice> = Vec::new();
    let mut default_effort: Option<String> = None;

    if let Some(options) = config_options {
        if let Some(thought) = options
            .iter()
            .find(|opt| opt["id"] == "thought_level" || opt["category"] == "thought_level")
        {
            default_effort = thought["currentValue"].as_str().map(str::to_owned);
            for choice in thought["options"].as_array().into_iter().flatten() {
                if let (Some(id), Some(name)) = (choice["value"].as_str(), choice["name"].as_str())
                {
                    effort_labels.insert(id.to_owned(), name.to_owned());
                    global_efforts.push(ProviderChoice {
                        id: id.to_owned(),
                        name: name.to_owned(),
                    });
                }
            }
        }
    }

    let available_model_ids: Vec<&str> = result["models"]["availableModels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["modelId"].as_str())
        .collect();

    let mut default_model: Option<String> = None;
    let mut models: Vec<ProviderModelChoice> = Vec::new();

    if let Some(model_opt) = config_options.and_then(|opts| {
        opts.iter()
            .find(|opt| opt["id"] == "model" || opt["category"] == "model")
    }) {
        default_model = model_opt["currentValue"].as_str().map(str::to_owned);
        for choice in model_opt["options"].as_array().into_iter().flatten() {
            let (Some(id), Some(name)) = (choice["value"].as_str(), choice["name"].as_str()) else {
                continue;
            };
            let prefix = format!("{id}-");
            let mut efforts: Vec<ProviderChoice> = Vec::new();
            for candidate in &available_model_ids {
                if let Some(suffix) = candidate.strip_prefix(&prefix) {
                    if let Some(label) = effort_labels.get(suffix) {
                        if !efforts.iter().any(|e| e.id == suffix) {
                            efforts.push(ProviderChoice {
                                id: suffix.to_owned(),
                                name: label.clone(),
                            });
                        }
                    }
                }
            }
            if efforts.is_empty() && !available_model_ids.contains(&id) {
                efforts = global_efforts.clone();
            }
            let model_default_effort = default_effort
                .as_ref()
                .filter(|d| efforts.iter().any(|e| &e.id == *d))
                .cloned()
                .or_else(|| efforts.first().map(|e| e.id.clone()));
            models.push(ProviderModelChoice {
                id: id.to_owned(),
                name: name.to_owned(),
                provider: None,
                context_window: None,
                efforts,
                default_effort: model_default_effort,
            });
        }
    } else if !available_model_ids.is_empty() {
        default_model = result["models"]["currentModelId"]
            .as_str()
            .map(str::to_owned);
        for item in result["models"]["availableModels"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let (Some(id), Some(name)) = (item["modelId"].as_str(), item["name"].as_str()) {
                models.push(ProviderModelChoice {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    provider: None,
                    context_window: None,
                    efforts: Vec::new(),
                    default_effort: None,
                });
            }
        }
    }

    let agents: Vec<ProviderChoice> = result["modes"]["availableModes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|mode| {
            let id = mode["id"].as_str()?;
            let name = mode["name"].as_str().unwrap_or(id);
            Some(ProviderChoice {
                id: id.to_owned(),
                name: name.to_owned(),
            })
        })
        .collect();

    ProviderChoices {
        models,
        agents,
        default_model,
    }
}

pub(super) fn parse_current_options(result: &Value) -> SessionOptionState {
    let mut state = SessionOptionState::default();
    if let Some(options) = result["configOptions"].as_array() {
        for opt in options {
            let value = opt["currentValue"].as_str().map(str::to_owned);
            match opt["id"].as_str() {
                Some("model") => state.model = value,
                Some("thought_level") => state.effort = value,
                Some("mode") => state.agent = value,
                _ => {}
            }
        }
    }
    if state.model.is_none() {
        state.model = result["models"]["currentModelId"]
            .as_str()
            .map(str::to_owned);
    }
    if state.agent.is_none() {
        state.agent = result["modes"]["currentModeId"].as_str().map(str::to_owned);
    }
    state
}

pub(super) fn parse_available_commands(update: &Value) -> Vec<OpenCodeCommandInfo> {
    update["availableCommands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let name = item["name"].as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(OpenCodeCommandInfo {
                name: name.to_owned(),
                description: item["description"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

pub(super) fn prompt_blocks(prompt: &str) -> Vec<Value> {
    let (body, attachments) = crate::application::split_context(prompt);
    let mut blocks = Vec::new();
    let mut text = body.to_owned();
    for attachment in attachments {
        if attachment.kind == crate::application::AttachmentKind::Image {
            if let Some((mime, data)) = parse_data_url(&attachment.content) {
                blocks.push(json!({
                    "type": "image",
                    "mimeType": mime,
                    "data": data,
                }));
                continue;
            }
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(&format!(
            "Attached context ({}):\n{}",
            attachment.path, attachment.content
        ));
    }
    blocks.insert(0, json!({ "type": "text", "text": text }));
    blocks
}

fn parse_data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (header, data) = rest.split_once(',')?;
    let mime = header.strip_suffix(";base64")?;
    if mime.is_empty() || data.is_empty() {
        return None;
    }
    Some((mime, data))
}

/// Convert replayed ACP `session/update` payloads into normalized history items.
pub(super) fn history_from_updates(
    updates: &[Value],
    is_working: bool,
    opts: &SessionOptionState,
) -> SessionHistoryPage {
    let mut data: Vec<Value> = Vec::new();
    let mut tool_positions: HashMap<String, usize> = HashMap::new();
    let mut counter: u64 = 0;

    for update in updates {
        let kind = update["sessionUpdate"].as_str().unwrap_or_default();
        match kind {
            "user_message_chunk" => {
                let text = chunk_text(update);
                if text.is_empty() {
                    continue;
                }
                if let Some(last) = data.last_mut().filter(|item| item["type"] == "user") {
                    if let Some(existing) = last["text"].as_str().map(str::to_owned) {
                        last["text"] = Value::String(format!("{existing}{text}"));
                    }
                } else {
                    counter += 1;
                    data.push(json!({
                        "id": format!("acp-user-{counter}"),
                        "type": "user",
                        "text": text,
                    }));
                }
            }
            "agent_message_chunk" => {
                let text = chunk_text(update);
                if text.is_empty() {
                    continue;
                }
                if let Some(last) = data.last_mut().filter(|item| item["type"] == "assistant") {
                    if let Some(existing) = last["text"].as_str().map(str::to_owned) {
                        last["text"] = Value::String(format!("{existing}{text}"));
                    }
                } else {
                    counter += 1;
                    data.push(json!({
                        "id": format!("acp-assistant-{counter}"),
                        "type": "assistant",
                        "text": text,
                    }));
                }
            }
            "agent_thought_chunk" => {
                let text = chunk_text(update);
                if text.is_empty() {
                    continue;
                }
                if let Some(last) = data.last_mut().filter(|item| item["type"] == "reasoning") {
                    if let Some(existing) = last["text"].as_str().map(str::to_owned) {
                        last["text"] = Value::String(format!("{existing}{text}"));
                    }
                } else {
                    counter += 1;
                    data.push(json!({
                        "id": format!("acp-reasoning-{counter}"),
                        "type": "reasoning",
                        "text": text,
                    }));
                }
            }
            "tool_call" | "tool_call_update" => {
                let Some(tool_id) = update["toolCallId"].as_str().filter(|s| !s.is_empty()) else {
                    continue;
                };
                if let Some(&pos) = tool_positions.get(tool_id) {
                    merge_tool_update(&mut data[pos], update);
                } else {
                    let mut item = json!({
                        "id": format!("acp-tool-{tool_id}"),
                        "type": "tool",
                        "toolCallId": tool_id,
                        "name": tool_name(update),
                        "title": update["title"].as_str().unwrap_or_default(),
                        "status": update["status"].as_str().unwrap_or("completed"),
                        "input": tool_input(update),
                        "output": tool_output(update),
                    });
                    merge_tool_update(&mut item, update);
                    tool_positions.insert(tool_id.to_owned(), data.len());
                    data.push(item);
                }
            }
            "usage_update" => {
                if let (Some(used), Some(size)) = (update["used"].as_u64(), update["size"].as_u64())
                {
                    data.push(json!({
                        "type": "usage",
                        "used": used,
                        "size": size,
                    }));
                }
            }
            _ => {}
        }
    }

    SessionHistoryPage {
        data,
        next_cursor: None,
        is_working: Some(is_working),
        model: opts.model.clone(),
        effort: opts.effort.clone(),
        agent: opts.agent.clone(),
    }
}

pub(crate) fn chunk_text(update: &Value) -> String {
    let content = &update["content"];
    if let Some(text) = content["text"].as_str() {
        return text.to_owned();
    }
    if let Some(text) = update["text"].as_str() {
        return text.to_owned();
    }
    String::new()
}

pub(crate) fn tool_name(update: &Value) -> String {
    update["name"]
        .as_str()
        .or_else(|| update["title"].as_str())
        .or_else(|| update["kind"].as_str())
        .unwrap_or("tool")
        .to_owned()
}

pub(crate) fn tool_input(update: &Value) -> String {
    let raw = if !update["rawInput"].is_null() {
        &update["rawInput"]
    } else if !update["args"].is_null() {
        &update["args"]
    } else {
        return String::new();
    };
    if let Some(text) = raw.as_str() {
        text.to_owned()
    } else {
        serde_json::to_string_pretty(raw).unwrap_or_default()
    }
}

pub(crate) fn tool_output(update: &Value) -> String {
    if let Some(items) = update["content"].as_array() {
        let mut out = String::new();
        for item in items {
            let text = item["content"]["text"]
                .as_str()
                .or_else(|| item["text"].as_str());
            if let Some(text) = text {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(text);
            }
        }
        if !out.is_empty() {
            return out;
        }
    }
    if !update["rawOutput"].is_null() {
        if let Some(text) = update["rawOutput"].as_str() {
            return text.to_owned();
        }
        return serde_json::to_string_pretty(&update["rawOutput"]).unwrap_or_default();
    }
    String::new()
}

fn merge_tool_update(target: &mut Value, update: &Value) {
    if let Some(name) = update["name"].as_str().filter(|s| !s.is_empty()) {
        target["name"] = Value::String(name.to_owned());
    }
    if let Some(title) = update["title"].as_str().filter(|s| !s.is_empty()) {
        target["title"] = Value::String(title.to_owned());
    }
    if let Some(status) = update["status"].as_str().filter(|s| !s.is_empty()) {
        target["status"] = Value::String(status.to_owned());
    }
    let input = tool_input(update);
    if !input.is_empty() {
        target["input"] = Value::String(input);
    }
    let output = tool_output(update);
    if !output.is_empty() {
        target["output"] = Value::String(output);
    }
}

fn parse_timestamp_ms(value: &Value) -> Option<u64> {
    if let Some(ms) = value.as_u64() {
        return Some(ms);
    }
    let text = value.as_str()?;
    let (date, rest) = text.split_once('T')?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let time_part = rest.strip_suffix('Z').unwrap_or(rest);
    let time_core = time_part.split(['+', '-']).next()?;
    let mut t = time_core.split(':');
    let hour: u64 = t.next()?.parse().ok()?;
    let minute: u64 = t.next()?.parse().ok()?;
    let sec_str = t.next()?;
    let (sec, frac) = sec_str.split_once('.').unwrap_or((sec_str, ""));
    let second: u64 = sec.parse().ok()?;
    let millis: u64 = if frac.is_empty() {
        0
    } else {
        let mut buf = frac.chars().take(3).collect::<String>();
        while buf.len() < 3 {
            buf.push('0');
        }
        buf.parse().ok()?
    };
    let days = days_from_civil(year, month, day);
    let secs = days.checked_mul(86_400)? + (hour * 3600 + minute * 60 + second) as i64;
    u64::try_from(secs)
        .ok()?
        .checked_mul(1000)?
        .checked_add(millis)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = u64::from(month);
    let d = u64::from(day);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i64 - 719_468
}
