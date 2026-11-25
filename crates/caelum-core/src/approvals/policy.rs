use caelum_protocol::{ApprovalDecision, ApprovalRule, PermissionRequest};
use serde_json::Value;
use std::path::Path;

#[cfg(test)]
mod tests;

pub(super) fn classify(req: &PermissionRequest) -> Option<ApprovalRule> {
    let session = req.session_id.as_deref().filter(|s| !s.is_empty())?;
    let details = &req.provider_details;
    match req.provider.as_str() {
        "codex" if details["method"] == "item/commandExecution/requestApproval" => {
            codex_command(&req.project_path, session, &details["params"])
        }
        "opencode" => {
            let p = &details["request"];
            let [path] = p["resources"].as_array()?.as_slice() else {
                return None;
            };
            if p["action"] != "read"
                || p["sessionID"] != session
                || !only_fields(p, &["id", "sessionID", "action", "resources", "message"])
            {
                return None;
            }
            read_file(&req.project_path, path.as_str()?)
        }
        "antigravity" if details["sessionId"] == session => {
            crate::antigravity::protocol::permission_option(details, ApprovalDecision::AllowOnce)?;
            acp_read(&req.project_path, &details["toolCall"])
        }
        _ => None,
    }
}

fn only_fields(value: &Value, allowed: &[&str]) -> bool {
    value.as_object().is_some_and(|fields| {
        fields
            .iter()
            .all(|(k, v)| v.is_null() || allowed.contains(&k.as_str()))
    })
}

fn codex_command(root: &str, session: &str, p: &Value) -> Option<ApprovalRule> {
    if p["threadId"] != session
        || p["cwd"] != root
        || !only_fields(
            p,
            &[
                "threadId",
                "turnId",
                "itemId",
                "approvalId",
                "reason",
                "cwd",
                "command",
                "commandActions",
                "availableDecisions",
            ],
        )
        || !p["availableDecisions"].is_null()
            && !p["availableDecisions"]
                .as_array()?
                .iter()
                .any(|v| v == "accept")
    {
        return None;
    }
    let allowed = [
        "pwd",
        "ls",
        "ls -l",
        "git status",
        "git status --short",
        "cargo test",
        "cargo test --locked",
        "cargo test --workspace",
        "cargo test --workspace --locked",
    ];
    let cmd = allowed.into_iter().find(|&cmd| match &p["command"] {
        Value::String(s) => s == cmd,
        Value::Array(args) => args.iter().map(Value::as_str).eq(cmd.split(' ').map(Some)),
        _ => false,
    })?;
    if !p["commandActions"].is_null()
        && p["commandActions"]
            .as_array()?
            .iter()
            .any(|a| a["command"] != cmd)
    {
        return None;
    }
    Some(ApprovalRule::ExactCommand(cmd.into()))
}

fn acp_read(root: &str, tool: &Value) -> Option<ApprovalRule> {
    if tool["kind"] != "read"
        || tool
            .get("name")
            .is_some_and(|v| !matches!(v.as_str(), Some("read" | "read_file" | "view_file")))
    {
        return None;
    }
    let input = tool["rawInput"].as_object()?;
    if input.len() != 1 {
        return None;
    }
    let (key, value) = input.iter().next()?;
    if !matches!(
        key.as_str(),
        "path" | "file_path" | "filePath" | "AbsolutePath"
    ) {
        return None;
    }
    let rule = read_file(root, value.as_str()?)?;
    if let Some(locations) = tool.get("locations") {
        for loc in locations.as_array()? {
            if read_file(root, loc["path"].as_str()?)? != rule {
                return None;
            }
        }
    }
    Some(rule)
}

fn read_file(root: &str, resource: &str) -> Option<ApprovalRule> {
    let path = Path::new(resource);
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let name = path.to_str()?;
    if name.len() > 512
        || resource.ends_with('/')
        || name.split('/').any(|part| matches!(part, "" | "." | ".."))
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"/._- ".contains(&c))
    {
        return None;
    }
    Some(ApprovalRule::ReadFile(name.into()))
}
