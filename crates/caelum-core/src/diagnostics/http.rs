use super::{next, Span};

pub(crate) fn http(method: &reqwest::Method, path: &str) -> Option<Span> {
    if !super::enabled() {
        return None;
    }
    next("http", route(method, path), 0).map(|mark| mark.span())
}

fn route(method: &reqwest::Method, path: &str) -> &'static str {
    match path {
        "/api/info" => "info",
        "/api/session/active" => "sessions.active",
        "/api/session" if *method == reqwest::Method::GET => "sessions.list",
        "/api/session" => "sessions.create",
        "/api/model" => "models",
        "/api/model/default" => "model.default",
        "/api/agent" => "agents",
        "/api/provider" => "providers",
        "/api/command" => "commands",
        "/api/form" => "forms",
        "/api/shell" => "shells",
        _ => match path
            .strip_prefix("/api/session/")
            .and_then(|s| s.split_once('/'))
        {
            Some((_, "message")) => "session.messages",
            Some((_, "prompt")) => "session.prompt",
            Some((_, "diff")) => "session.diff",
            Some((_, "inbox")) => "session.inbox",
            Some((_, "agent")) => "session.agent",
            Some((_, "model")) => "session.model",
            Some((_, "interrupt")) => "session.interrupt",
            None if path.starts_with("/api/session/") => "session.metadata",
            _ => "other",
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn routes_never_include_session_ids_or_unknown_paths() {
        for (path, label) in [
            ("/api/session/private-session/message", "session.messages"),
            ("/api/session/private-session", "session.metadata"),
            ("/api/custom/private-project", "other"),
            ("/api/session/private-session/private-extension", "other"),
        ] {
            assert_eq!(super::route(&reqwest::Method::GET, path), label);
        }
    }
}
