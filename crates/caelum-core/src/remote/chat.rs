use serde::{Deserialize, Serialize};

const HANDOFF_HEADER: &str =
    "Conversation continued from Caelum. Previous messages (context only):";

/// Only visible user/assistant text crosses from a local provider to Boat.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chat {
    pub messages: Vec<Message>,
    pub clipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Message {
    User(String),
    Assistant(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Continuation {
    /// Stable editor identity, including before Boat assigns a conversation ID.
    pub id: String,
    pub title: String,
    pub chat: Chat,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<super::PublishedBase>,
}

impl Chat {
    /// A thread preview can stop inside the header, before any of the new user's message.
    pub(crate) fn is_handoff_preview(label: &str) -> bool {
        label.starts_with(HANDOFF_HEADER)
            || (label.starts_with("Conversation continued from Caelum")
                && HANDOFF_HEADER.starts_with(label.trim_end_matches('…').trim_end_matches("...")))
    }

    pub(crate) fn prompt(&self, text: &str) -> String {
        if self.messages.is_empty() {
            return text.into();
        }
        let history = serde_json::to_string(&self.messages).expect("chat text is serializable");
        format!("{HANDOFF_HEADER}\n{history}\n\nNext user message:\n{text}")
    }
}
