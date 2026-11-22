use serde_json::Value;

pub(super) fn executable(command: &str) -> Option<&str> {
    let word = command.split_whitespace().next()?;
    if !word
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"/._-+".contains(&b))
    {
        return None;
    }
    let program = word.rsplit('/').next()?;
    (!program.is_empty() && program.len() <= 40).then_some(program)
}

/// A compact tool target suitable for a conversation preview: an executable or checkout-relative path.
pub(crate) fn tool_detail(name: &str, input: &Value, root: Option<&str>) -> Option<String> {
    match name.to_ascii_lowercase().as_str() {
        "bash" | "shell" | "exec_command" | "commandexecution" | "execute" => executable(
            input
                .get("command")
                .or_else(|| input.get("cmd"))?
                .as_str()?,
        )
        .map(str::to_owned),
        "read" | "read_file" | "edit" | "write" | "grep" | "glob" | "filechange" => {
            let input = input
                .as_array()
                .and_then(|changes| changes.first())
                .unwrap_or(input);
            let path = ["filePath", "file_path", "path"]
                .iter()
                .find_map(|p| input.get(p)?.as_str())?;
            let path = if path.starts_with('/') {
                path.strip_prefix(root?)?.strip_prefix('/')?
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

pub(super) fn tool_action(name: &str, detail: Option<&str>) -> String {
    let (verb, fallback) = match name.to_ascii_lowercase().as_str() {
        "read" | "read_file" => ("Reading", "Reading a file"),
        "edit" | "filechange" | "apply_patch" | "patch" => ("Editing", "Editing files"),
        "write" => ("Writing", "Writing a file"),
        "grep" | "glob" | "search" | "websearch" => ("Searching", "Searching"),
        "bash" | "shell" | "exec_command" | "commandexecution" | "execute" | "background shell" => {
            ("Running", "Running a command")
        }
        _ => return format!("Using {}", super::excerpt(name)),
    };
    detail.map_or_else(|| fallback.into(), |detail| format!("{verb} {detail}"))
}
