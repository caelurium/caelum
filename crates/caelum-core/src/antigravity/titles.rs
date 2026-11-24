use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    process::{Command, Stdio},
};

use caelum_protocol::{AntigravityEvent, ProviderChoices};
use serde_json::{json, Value};

use super::{lock, protocol, transport, AntigravityError, AntigravityProvider, SessionOptionState};
use crate::projects::Project;

impl AntigravityProvider {
    pub fn rename(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
        title: &str,
    ) -> Result<(), AntigravityError> {
        if session_id.trim().is_empty() {
            return Err(AntigravityError::Protocol(
                "Antigravity session ID cannot be empty".into(),
            ));
        }
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        self.verify_session_owned(&connection, &project, session_id)?;
        lock(&self.inner.metadata).set_title(project.path(), session_id, title)
    }

    /// Generate a fresh title for an Antigravity session from its recent
    /// conversation, persist it in local metadata, and emit `session_info_update`.
    pub fn regenerate_title(
        &self,
        project_path: impl AsRef<Path>,
        session_id: &str,
    ) -> Result<(), AntigravityError> {
        if session_id.trim().is_empty() {
            return Err(AntigravityError::Protocol(
                "Antigravity session ID cannot be empty".into(),
            ));
        }
        let project = Project::open(project_path)?;
        let connection = self.connection(&project)?;
        self.verify_session_owned(&connection, &project, session_id)?;

        let excerpt = match lock(&connection.transcripts).format_excerpt(session_id) {
            Some(excerpt) => excerpt,
            None => {
                if lock(&connection.progress).is_working(session_id) {
                    return Err(AntigravityError::Protocol(
                        "Antigravity session has no recorded transcript yet".into(),
                    ));
                }
                let (_, replayed) = self.load_session(&connection, project.path(), session_id)?;
                let page = protocol::history_from_updates(
                    &replayed,
                    false,
                    &SessionOptionState::default(),
                    Some(project.path()),
                );
                let mut transcripts = lock(&connection.transcripts);
                transcripts.record_history(session_id, &page.data);
                transcripts.format_excerpt(session_id).ok_or_else(|| {
                    AntigravityError::Protocol("No conversation messages to title".into())
                })?
            }
        };

        let model = fast_model(lock(&connection.choices).as_ref());
        let executable = self.executable()?;
        let title = generate_title(&executable, &excerpt, model.as_ref())?;

        {
            let mut metadata = lock(&self.inner.metadata);
            if metadata.is_deleted(project.path(), session_id) {
                return Err(AntigravityError::Protocol(
                    "Antigravity session was deleted".into(),
                ));
            }
            metadata.set_title(project.path(), session_id, &title)?;
        }
        lock(&connection.session_titles).insert(session_id.to_owned(), title.clone());
        transport::publish_event(
            &self.inner.subscribers,
            AntigravityEvent {
                project_path: project.path().to_string_lossy().into_owned(),
                session_id: Some(session_id.to_owned()),
                method: "session_info_update".into(),
                params: json!({
                    "sessionUpdate": "session_info_update",
                    "title": title,
                }),
            },
        );
        Ok(())
    }
}

const MAX_TURNS: usize = 12;
const MAX_TURN_CHARS: usize = 400;
const MAX_EXCERPT_CHARS: usize = 3_600;
const MAX_TITLE_CHARS: usize = 80;
const TITLE_PRINT_TIMEOUT: &str = "30s";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    User,
    Assistant,
}

impl Role {
    fn label(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Assistant => "Assistant",
        }
    }
}

struct Turn {
    role: Role,
    text: String,
}

/// Bounded per-session transcript excerpts kept in memory for dynamic title
/// generation without replaying `session/load` on an active ACP session.
#[derive(Default)]
pub(super) struct TranscriptStore {
    sessions: HashMap<String, VecDeque<Turn>>,
}

impl TranscriptStore {
    pub(super) fn record_history(&mut self, session_id: &str, data: &[Value]) {
        let mut turns = VecDeque::new();
        for item in data {
            let role = match item["type"].as_str() {
                Some("user") => Role::User,
                Some("assistant") => Role::Assistant,
                _ => continue,
            };
            let Some(text) = item["text"].as_str() else {
                continue;
            };
            let cleaned = if role == Role::User {
                crate::application::split_context(text).0.trim()
            } else {
                text.trim()
            };
            if cleaned.is_empty() {
                continue;
            }
            push_turn(&mut turns, role, cleaned);
        }
        if !turns.is_empty() {
            self.sessions.insert(session_id.to_owned(), turns);
        }
    }

    pub(super) fn record_user_prompt(&mut self, session_id: &str, prompt: &str) {
        let (body, _) = crate::application::split_context(prompt);
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return;
        }
        let turns = self.sessions.entry(session_id.to_owned()).or_default();
        push_turn(turns, Role::User, trimmed);
    }

    pub(super) fn record_chunk(&mut self, session_id: &str, is_user: bool, text: &str) {
        if text.is_empty() {
            return;
        }
        let role = if is_user { Role::User } else { Role::Assistant };
        let turns = self.sessions.entry(session_id.to_owned()).or_default();
        if let Some(last) = turns.back_mut().filter(|t| t.role == role) {
            append_capped(&mut last.text, text, MAX_TURN_CHARS);
        } else {
            let trimmed = text.trim_start();
            if trimmed.is_empty() {
                return;
            }
            push_turn(turns, role, trimmed);
        }
    }

    pub(super) fn remove(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    pub(super) fn format_excerpt(&self, session_id: &str) -> Option<String> {
        let turns = self.sessions.get(session_id)?;
        let mut lines = Vec::new();
        let mut total = 0usize;
        for turn in turns.iter().rev() {
            let text = turn.text.trim();
            if text.is_empty() {
                continue;
            }
            let line = format!("{}: {text}", turn.role.label());
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
}

fn push_turn(turns: &mut VecDeque<Turn>, role: Role, text: &str) {
    let mut stored = String::new();
    append_capped(&mut stored, text, MAX_TURN_CHARS);
    if stored.trim().is_empty() {
        return;
    }
    if turns.len() >= MAX_TURNS {
        turns.pop_front();
    }
    turns.push_back(Turn { role, text: stored });
}

fn append_capped(target: &mut String, addition: &str, limit: usize) {
    let current = target.chars().count();
    if current >= limit {
        return;
    }
    let remaining = limit - current;
    for ch in addition.chars().take(remaining) {
        target.push(ch);
    }
}

pub(super) fn fast_model(choices: Option<&ProviderChoices>) -> Option<(String, Option<String>)> {
    let choices = choices?;
    let model = choices
        .models
        .iter()
        .find(|m| m.id.to_ascii_lowercase().contains("flash"))?;
    let effort = model
        .efforts
        .iter()
        .find(|e| e.id == "low")
        .or_else(|| model.efforts.first())
        .map(|e| e.id.clone());
    Some((model.id.clone(), effort))
}

pub(super) fn generate_title(
    executable: &Path,
    excerpt: &str,
    model: Option<&(String, Option<String>)>,
) -> Result<String, AntigravityError> {
    let prompt = format!(
        "Generate a concise, descriptive session title (3 to 7 words, no quotes, no trailing punctuation, no markdown, single line) that captures the current topic of this conversation. Reply with ONLY the title.\n\nConversation:\n{excerpt}"
    );
    let mut command = Command::new(executable);
    command
        .args([
            "--print",
            &prompt,
            "--disable-slash-commands",
            "--output-format",
            "json",
            "--print-timeout",
            TITLE_PRINT_TIMEOUT,
        ])
        .current_dir(std::env::temp_dir())
        .env("PATH", crate::executable::path(executable))
        .stdin(Stdio::null());
    if let Some((model_id, effort)) = model {
        command.args(["--model", model_id]);
        if let Some(effort) = effort {
            command.args(["--effort", effort]);
        }
    }
    let output = command.output().map_err(|error| {
        AntigravityError::Launch(format!(
            "Could not run `{}` --print for title generation: {error}",
            executable.display()
        ))
    })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(AntigravityError::Protocol(if stderr.is_empty() {
            format!(
                "Antigravity title generation exited with status {}",
                output.status
            )
        } else {
            stderr
        }));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim()).map_err(|error| {
        AntigravityError::Protocol(format!("Invalid Antigravity title JSON: {error}"))
    })?;
    let raw = parsed["response"].as_str().ok_or_else(|| {
        AntigravityError::Protocol("Antigravity title response has no text".into())
    })?;
    sanitize_title(raw)
        .ok_or_else(|| AntigravityError::Protocol("Antigravity title response was empty".into()))
}

pub(super) fn sanitize_title(raw: &str) -> Option<String> {
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
    let trimmed = collapsed.trim().trim_end_matches('.').trim();
    if trimmed.is_empty() {
        return None;
    }
    let truncated: String = trimmed.chars().take(MAX_TITLE_CHARS).collect();
    let final_title = truncated.trim();
    (!final_title.is_empty()).then(|| final_title.to_owned())
}

#[cfg(test)]
mod tests {
    use caelum_protocol::{ProviderChoice, ProviderChoices, ProviderModelChoice};

    use super::{fast_model, sanitize_title, TranscriptStore};

    #[test]
    fn sanitizes_markdown_quotes_and_multiline_output() {
        assert_eq!(
            sanitize_title("\n  \"Fix Rust Borrow Checker Error.\"\nExtra line"),
            Some("Fix Rust Borrow Checker Error".into())
        );
        assert_eq!(
            sanitize_title("## Title: `Refactor   Session\tTitles`"),
            Some("Refactor Session Titles".into())
        );
        assert_eq!(sanitize_title("   \n\"\"  "), None);
    }

    #[test]
    fn picks_flash_model_with_low_effort_when_available() {
        let choices = ProviderChoices {
            default_model: Some("gemini-4-neon".into()),
            agents: Vec::new(),
            models: vec![
                ProviderModelChoice {
                    id: "gemini-4-neon".into(),
                    name: "Gemini 4 Neon".into(),
                    provider: None,
                    context_window: None,
                    efforts: vec![ProviderChoice {
                        id: "high".into(),
                        name: "High".into(),
                    }],
                    default_effort: Some("high".into()),
                },
                ProviderModelChoice {
                    id: "gemini-3.8-flash".into(),
                    name: "Gemini 3.8 Flash".into(),
                    provider: None,
                    context_window: None,
                    efforts: vec![
                        ProviderChoice {
                            id: "medium".into(),
                            name: "Medium".into(),
                        },
                        ProviderChoice {
                            id: "low".into(),
                            name: "Low".into(),
                        },
                    ],
                    default_effort: Some("medium".into()),
                },
            ],
        };
        assert_eq!(
            fast_model(Some(&choices)),
            Some(("gemini-3.8-flash".into(), Some("low".into())))
        );
    }

    #[test]
    fn transcript_store_records_and_formats_recent_turns() {
        let mut store = TranscriptStore::default();
        store.record_user_prompt("s1", "Fix the build");
        store.record_chunk("s1", false, "Checking ");
        store.record_chunk("s1", false, "Cargo.toml.");
        assert_eq!(
            store.format_excerpt("s1").as_deref(),
            Some("User: Fix the build\nAssistant: Checking Cargo.toml.")
        );
    }
}
