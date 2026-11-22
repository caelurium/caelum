use serde::{Deserialize, Serialize};

/// Visible user and assistant text shared when continuing with another backend.
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
    pub(crate) fn prompt(&self, text: &str) -> String {
        if self.messages.is_empty() {
            return text.into();
        }
        let history = serde_json::to_string(&self.messages).expect("chat text is serializable");
        format!("Conversation continued from Caelum. Previous messages (context only):\n{history}\n\nNext user message:\n{text}")
    }
}

/// Accumulates newest-first messages without retaining more than a handoff can send.
#[derive(Default)]
pub(crate) struct RecentChat {
    messages: Vec<Message>,
    bytes: usize,
    clipped: bool,
}

impl RecentChat {
    pub(crate) fn push(&mut self, mut message: Message) -> bool {
        const MAX_BYTES: usize = 64 * 1024;
        let (Message::User(text) | Message::Assistant(text)) = &mut message;
        if self.messages.len() == 200 || text.len() > MAX_BYTES - self.bytes {
            if self.messages.is_empty() {
                text.drain(..text.ceil_char_boundary(text.len() - MAX_BYTES));
                self.messages.push(message);
            }
            self.clipped = true;
            return false;
        }
        self.bytes += text.len();
        self.messages.push(message);
        true
    }

    pub(crate) fn finish(mut self, more: bool) -> Chat {
        self.messages.reverse();
        Chat {
            messages: self.messages,
            clipped: self.clipped || more,
        }
    }
}
