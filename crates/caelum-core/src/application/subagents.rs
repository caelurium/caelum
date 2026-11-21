//! Provider-owned subagent identity and readable task details.
use serde_json::Value;

use super::{Entry, EntryKind};

pub fn running(entries: &[Entry]) -> usize {
    entries
        .iter()
        .filter(|entry| entry.name == "subagent" && !entry.complete())
        .map(|entry| child_id(entry).unwrap_or(&entry.id))
        .collect::<std::collections::HashSet<_>>()
        .len()
}

pub struct Subagent {
    pub agent: String,
    pub description: String,
    pub prompt: String,
    pub session_id: Option<String>,
    pub model: Option<String>,
}

impl Subagent {
    pub fn from_entry(entry: &Entry) -> Option<Self> {
        if entry.kind() != EntryKind::Tool || entry.name != "subagent" {
            return None;
        }
        let input: Value = serde_json::from_str(&entry.input).unwrap_or_default();
        Some(Self {
            agent: input["agent"].as_str().unwrap_or("Subagent").into(),
            description: input["description"]
                .as_str()
                .unwrap_or("Delegated task")
                .into(),
            prompt: input["prompt"].as_str().unwrap_or_default().into(),
            session_id: child_id(entry).map(str::to_owned).or_else(|| {
                input["sessionID"]
                    .as_str()
                    .filter(|id| valid_id(id))
                    .map(str::to_owned)
            }),
            model: input["model"]
                .as_str()
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
        })
    }
}

pub(crate) fn child_id(entry: &Entry) -> Option<&str> {
    (entry.kind() == EntryKind::Tool && entry.name == "subagent")
        .then(|| entry.metadata["sessionID"].as_str())
        .flatten()
        .filter(|id| valid_id(id))
}

pub(crate) fn valid_id(id: &str) -> bool {
    id.starts_with("ses_")
        && id.len() > 4
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
