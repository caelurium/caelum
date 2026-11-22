use std::path::Path;

use caelum_protocol::LineChanges;
use serde_json::{json, Map, Value};

mod diff;
use diff::build_patch;

#[derive(Debug, Clone)]
pub(crate) struct NormalizedTool {
    pub name: String,
    pub status: String,
    pub input: String,
    pub output: String,
    pub error: String,
    pub metadata: Value,
}

/// Merge an ACP `tool_call` or `tool_call_update` payload into the accumulated
/// raw state for a `toolCallId` so later partial updates retain earlier fields.
pub(crate) fn merge_raw_tool(target: &mut Value, update: &Value) {
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let Some(map) = target.as_object_mut() else {
        return;
    };
    for key in ["toolCallId", "name", "kind", "title", "status"] {
        if let Some(value) = update[key].as_str().filter(|s| !s.is_empty()) {
            map.insert(key.to_owned(), Value::String(value.to_owned()));
        }
    }
    if let Some(exit) = update["exitCode"].as_i64() {
        map.insert("exitCode".into(), json!(exit));
    }
    if let Some(locations) = update["locations"].as_array().filter(|a| !a.is_empty()) {
        map.insert("locations".into(), Value::Array(locations.clone()));
    }
    if let Some(content) = update["content"].as_array().filter(|a| !a.is_empty()) {
        map.insert("content".into(), Value::Array(content.clone()));
    }
    if !update["rawOutput"].is_null() {
        map.insert("rawOutput".into(), update["rawOutput"].clone());
    }
    for key in ["args", "rawInput"] {
        let incoming = &update[key];
        if incoming.is_null() {
            continue;
        }
        if let (Some(existing_obj), Some(incoming_obj)) = (
            map.get_mut(key).and_then(Value::as_object_mut),
            incoming.as_object(),
        ) {
            for (k, v) in incoming_obj {
                if !v.is_null() {
                    existing_obj.insert(k.clone(), v.clone());
                }
            }
        } else {
            map.insert(key.to_owned(), incoming.clone());
        }
    }
}

/// Normalize an accumulated ACP tool payload into Caelum's canonical `Entry` fields.
pub(crate) fn normalize_tool(raw: &Value, project_path: Option<&Path>) -> NormalizedTool {
    let raw_name = raw["name"].as_str().unwrap_or_default().trim();
    let kind = raw["kind"].as_str().unwrap_or_default().trim();
    let title = raw["title"].as_str().unwrap_or_default().trim();
    let status = raw["status"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or("completed")
        .to_owned();
    let args = parse_args(raw);
    let has_diff_content = raw["content"]
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item["type"] == "diff"));

    let mut metadata = Map::new();
    metadata.insert("acp".into(), raw.clone());

    if is_edit_tool(raw_name, kind, has_diff_content) {
        let (path, patch) = build_patch(raw, &args, title, project_path);
        let mut input_obj = Map::new();
        if !path.is_empty() {
            input_obj.insert("path".into(), Value::String(path));
        }
        if !patch.is_empty() {
            input_obj.insert("patch".into(), Value::String(patch));
        } else if !title.is_empty() {
            input_obj.insert("description".into(), Value::String(title.to_owned()));
        }
        let (mut output, error, _) = extract_output(raw, OutputKind::Edit);
        if status != "failed" && error.is_empty() {
            output.clear();
        }
        let name = if matches!(raw_name, "write_file" | "write") {
            "write"
        } else {
            "edit"
        };
        return NormalizedTool {
            name: name.into(),
            status,
            input: Value::Object(input_obj).to_string(),
            output,
            error,
            metadata: Value::Object(metadata),
        };
    }

    if matches!(
        raw_name,
        "run_command" | "bash" | "shell" | "execute" | "commandExecution"
    ) || kind == "execute"
    {
        let (command, cwd) = extract_command_and_cwd(&args, title, project_path);
        let description = args_str(&args, &["toolSummary", "description", "Description"]);
        let mut input_obj = Map::new();
        if let Some(cmd) = command.filter(|c| !c.is_empty()) {
            input_obj.insert("command".into(), Value::String(cmd));
        } else if let Some(desc) = description.as_deref().filter(|d| !d.is_empty()) {
            input_obj.insert("description".into(), Value::String(desc.to_owned()));
        } else if !title.is_empty() {
            input_obj.insert("command".into(), Value::String(title.to_owned()));
        }
        if let Some(cwd) = cwd {
            input_obj.insert("cwd".into(), Value::String(cwd));
        }
        let (output, error, exit_code) = extract_output(raw, OutputKind::Execute);
        if let Some(exit) = exit_code {
            metadata.insert("exitCode".into(), json!(exit));
        }
        return NormalizedTool {
            name: "bash".into(),
            status,
            input: Value::Object(input_obj).to_string(),
            output,
            error,
            metadata: Value::Object(metadata),
        };
    }

    if matches!(raw_name, "read_file" | "view_file" | "read") || kind == "read" {
        let path = extract_path(raw, &args, title, project_path).unwrap_or_default();
        let mut input_obj = Map::new();
        if !path.is_empty() {
            input_obj.insert("path".into(), Value::String(path));
        }
        if let Some(offset) = args_u64(&args, &["StartLine", "start_line", "offset"]) {
            input_obj.insert("offset".into(), json!(offset));
            if let Some(end) = args_u64(&args, &["EndLine", "end_line"]) {
                input_obj.insert("limit".into(), json!(end.saturating_sub(offset) + 1));
            }
        }
        if !input_obj.contains_key("limit") {
            if let Some(limit) = args_u64(&args, &["limit", "Limit"]) {
                input_obj.insert("limit".into(), json!(limit));
            }
        }
        let (output, error, _) = extract_output(raw, OutputKind::Read);
        return NormalizedTool {
            name: "read_file".into(),
            status,
            input: Value::Object(input_obj).to_string(),
            output,
            error,
            metadata: Value::Object(metadata),
        };
    }

    if matches!(
        raw_name,
        "grep_search"
            | "find_by_name"
            | "list_dir"
            | "search_web"
            | "grep"
            | "glob"
            | "search"
            | "webSearch"
            | "websearch"
    ) || kind == "search"
    {
        let query = args_str(&args, &["Query", "query", "Pattern", "pattern"]);
        let path = args_str(
            &args,
            &[
                "SearchPath",
                "DirectoryPath",
                "SearchDirectory",
                "Path",
                "path",
            ],
        )
        .or_else(|| first_location_path(raw))
        .map(|p| clean_path(&p, project_path));
        let mut input_obj = Map::new();
        if let Some(q) = query.filter(|q| !q.is_empty()) {
            input_obj.insert("query".into(), Value::String(q));
        }
        if let Some(p) = path.filter(|p| !p.is_empty()) {
            input_obj.insert("path".into(), Value::String(p));
        }
        if input_obj.is_empty() && !title.is_empty() {
            input_obj.insert("query".into(), Value::String(title.to_owned()));
        }
        let (output, error, _) = extract_output(raw, OutputKind::Other);
        return NormalizedTool {
            name: "search".into(),
            status,
            input: Value::Object(input_obj).to_string(),
            output,
            error,
            metadata: Value::Object(metadata),
        };
    }

    if raw_name == "invoke_subagent" {
        let agent = args_str(&args, &["TypeName", "agent"]).unwrap_or_else(|| "Subagent".into());
        let description =
            args_str(&args, &["Role", "description"]).unwrap_or_else(|| "Delegated task".into());
        let prompt = args_str(&args, &["Prompt", "prompt"]).unwrap_or_default();
        let model = args_str(&args, &["Model", "model"]).filter(|m| m != "inherit");
        let mut input_obj = Map::new();
        input_obj.insert("agent".into(), Value::String(agent));
        input_obj.insert("description".into(), Value::String(description));
        if !prompt.is_empty() {
            input_obj.insert("prompt".into(), Value::String(prompt));
        }
        if let Some(model) = model {
            input_obj.insert("model".into(), Value::String(model));
        }
        let (output, error, _) = extract_output(raw, OutputKind::Other);
        return NormalizedTool {
            name: "subagent".into(),
            status,
            input: Value::Object(input_obj).to_string(),
            output,
            error,
            metadata: Value::Object(metadata),
        };
    }

    let name = if !raw_name.is_empty() {
        raw_name
    } else if !title.is_empty() {
        title
    } else if !kind.is_empty() {
        kind
    } else {
        "tool"
    }
    .to_owned();
    let input = if !args.is_null() {
        if let Some(text) = args.as_str() {
            text.to_owned()
        } else {
            serde_json::to_string_pretty(&args).unwrap_or_default()
        }
    } else if !title.is_empty() {
        json!({ "description": title }).to_string()
    } else {
        String::new()
    };
    let (output, error, _) = extract_output(raw, OutputKind::Other);
    NormalizedTool {
        name,
        status,
        input,
        output,
        error,
        metadata: Value::Object(metadata),
    }
}

/// Sum added and deleted diff lines across completed Antigravity edit/write history entries.
pub(crate) fn session_line_changes(items: &[Value]) -> Option<LineChanges> {
    let mut total = LineChanges::default();
    let mut any = false;
    for item in items {
        if item["type"] != "tool" {
            continue;
        }
        let name = item["name"].as_str().unwrap_or_default();
        if !matches!(name, "edit" | "write" | "patch" | "apply_patch") {
            continue;
        }
        let status = item["status"].as_str().unwrap_or_default();
        if matches!(status, "pending" | "in_progress" | "running" | "failed") {
            continue;
        }
        let input_str = item["input"].as_str().unwrap_or_default();
        let Ok(input) = serde_json::from_str::<Value>(input_str) else {
            continue;
        };
        let Some(patch) = input["patch"].as_str() else {
            continue;
        };
        any = true;
        for line in patch.lines() {
            if line.starts_with('+') && !line.starts_with("+++") {
                total.additions += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                total.deletions += 1;
            }
        }
    }
    any.then_some(total)
}

#[derive(Clone, Copy)]
enum OutputKind {
    Execute,
    Read,
    Edit,
    Other,
}

fn is_edit_tool(raw_name: &str, kind: &str, has_diff_content: bool) -> bool {
    has_diff_content
        || kind == "edit"
        || matches!(
            raw_name,
            "edit_file"
                | "write_file"
                | "replace_file_content"
                | "multi_replace_file_content"
                | "apply_patch"
                | "edit"
                | "write"
                | "patch"
                | "fileChange"
        )
}

fn parse_args(raw: &Value) -> Value {
    let val = if !raw["rawInput"].is_null() {
        &raw["rawInput"]
    } else {
        &raw["args"]
    };
    if let Some(s) = val.as_str() {
        serde_json::from_str::<Value>(s).unwrap_or_else(|_| Value::String(s.to_owned()))
    } else {
        val.clone()
    }
}

fn args_str(args: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = args[*key].as_str().filter(|s| !s.is_empty()) {
            return Some(value.to_owned());
        }
    }
    None
}

fn args_u64(args: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        if let Some(value) = args[*key].as_u64() {
            return Some(value);
        }
    }
    None
}

fn first_location_path(raw: &Value) -> Option<String> {
    raw["locations"]
        .as_array()?
        .first()
        .and_then(|loc| loc["path"].as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn extract_path(
    raw: &Value,
    args: &Value,
    title: &str,
    project_path: Option<&Path>,
) -> Option<String> {
    let raw_path = args_str(
        args,
        &["Path", "TargetFile", "path", "filePath", "file_path"],
    )
    .or_else(|| {
        raw["content"]
            .as_array()
            .and_then(|items| {
                items.iter().find_map(|item| {
                    (item["type"] == "diff")
                        .then(|| item["path"].as_str())
                        .flatten()
                })
            })
            .map(str::to_owned)
    })
    .or_else(|| first_location_path(raw))
    .or_else(|| {
        title
            .strip_prefix("File ")
            .or_else(|| title.strip_prefix("Read "))
            .or_else(|| title.strip_prefix("Edit "))
            .or_else(|| title.strip_prefix("Write "))
            .map(|s| s.trim().to_owned())
    })?;
    Some(clean_path(&raw_path, project_path))
}

pub(crate) fn clean_path(raw: &str, project_path: Option<&Path>) -> String {
    let trimmed = raw.trim().strip_prefix("file://").unwrap_or(raw.trim());
    if let Some(project) = project_path {
        if let Ok(rel) = Path::new(trimmed).strip_prefix(project) {
            let rel_str = rel.to_string_lossy();
            if !rel_str.is_empty() {
                return rel_str.into_owned();
            }
        }
    }
    trimmed.to_owned()
}

fn extract_command_and_cwd(
    args: &Value,
    title: &str,
    project_path: Option<&Path>,
) -> (Option<String>, Option<String>) {
    let mut command = args_str(args, &["CommandLine", "command", "cmd"]);
    let mut cwd = args_str(args, &["Cwd", "cwd", "workdir"]);

    if command.is_none() && !title.is_empty() && title != "Run shell command" {
        if let Some(rest) = title.strip_prefix("cd ") {
            if let Some((dir, cmd)) = rest.split_once(" && ") {
                if cwd.is_none() {
                    cwd = Some(dir.trim().to_owned());
                }
                command = Some(cmd.trim().to_owned());
            }
        }
    }

    if let (Some(project), Some(c)) = (project_path, cwd.as_deref()) {
        if Path::new(c) == project {
            cwd = None;
        }
    }
    (command, cwd)
}

fn extract_output(raw: &Value, kind: OutputKind) -> (String, String, Option<i64>) {
    let mut exit_code = raw["exitCode"]
        .as_i64()
        .or_else(|| raw["rawOutput"]["exitCode"].as_i64());
    let mut error = raw["rawOutput"]["error"]
        .as_str()
        .or_else(|| raw["error"].as_str())
        .unwrap_or_default()
        .to_owned();

    let mut text = String::new();
    if let Some(items) = raw["content"].as_array() {
        for item in items {
            if item["type"] == "diff" {
                continue;
            }
            if let Some(chunk) = item["content"]["text"]
                .as_str()
                .or_else(|| item["text"].as_str())
            {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(chunk);
            }
        }
    }
    if text.is_empty() && !raw["rawOutput"].is_null() {
        if let Some(s) = raw["rawOutput"].as_str() {
            text = s.to_owned();
        } else if let Some(s) = raw["rawOutput"]["output"].as_str() {
            text = s.to_owned();
        } else if raw["rawOutput"].is_object()
            && !raw["rawOutput"]["error"].is_null()
            && error == raw["rawOutput"]["error"].as_str().unwrap_or_default()
        {
            text = String::new();
        } else {
            text = serde_json::to_string_pretty(&raw["rawOutput"]).unwrap_or_default();
        }
    }

    let cleaned = strip_timestamps(&text);
    let final_output = match kind {
        OutputKind::Execute => {
            let (out, parsed_exit) = parse_execute_output(&cleaned);
            if exit_code.is_none() {
                exit_code = parsed_exit;
            }
            out
        }
        OutputKind::Read => clean_read_output(&cleaned),
        OutputKind::Edit | OutputKind::Other => cleaned,
    };

    if error == final_output {
        error.clear();
    }

    (final_output, error, exit_code)
}

fn strip_timestamps(text: &str) -> String {
    let mut rest = text;
    loop {
        if let Some(after) = rest.strip_prefix("Created At:") {
            rest = after.split_once('\n').map_or("", |(_, tail)| tail);
        } else if let Some(after) = rest.strip_prefix("Completed At:") {
            rest = after.split_once('\n').map_or("", |(_, tail)| tail);
        } else {
            break;
        }
    }
    rest.to_owned()
}

fn parse_execute_output(text: &str) -> (String, Option<i64>) {
    let mut exit_code = None;
    let mut rest = text;
    if let Some(after_prefix) = rest.strip_prefix("Command exited with code ") {
        if let Some((num_part, tail)) = after_prefix.split_once('\n') {
            if let Ok(code) = num_part.trim_end_matches('.').trim().parse::<i64>() {
                exit_code = Some(code);
                rest = tail.trim_start_matches('\n');
            }
        } else if let Ok(code) = after_prefix
            .trim_end_matches(['.', '\r', ' '])
            .parse::<i64>()
        {
            return (String::new(), Some(code));
        }
    }
    if let Some(after_header) = rest.strip_prefix("Command output:\n") {
        rest = after_header;
    } else if rest.trim() == "Command output:" {
        rest = "";
    }
    (rest.to_owned(), exit_code)
}

fn clean_read_output(text: &str) -> String {
    let rest = if let Some(after) = text.strip_prefix("Showing lines ") {
        after.split_once('\n').map_or(text, |(_, tail)| tail)
    } else {
        text
    };
    let lines: Vec<&str> = rest.lines().collect();
    if lines.is_empty() {
        return rest.to_owned();
    }
    let all_numbered = lines.iter().all(|line| {
        line.split_once(": ")
            .is_some_and(|(num, _)| !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()))
    });
    if all_numbered {
        lines
            .iter()
            .filter_map(|line| line.split_once(": ").map(|(_, body)| body))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        rest.to_owned()
    }
}
