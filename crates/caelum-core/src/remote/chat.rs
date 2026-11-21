use serde::{Deserialize, Serialize};

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
    pub(super) fn prompt(&self, text: &str) -> String {
        if self.messages.is_empty() {
            return text.into();
        }
        let history = serde_json::to_string(&self.messages).expect("chat text is serializable");
        format!("Conversation continued from Caelum. Previous messages (context only):\n{history}\n\nNext user message:\n{text}")
    }
}
