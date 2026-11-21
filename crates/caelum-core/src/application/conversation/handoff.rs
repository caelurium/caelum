use super::{Conversation, Entry, EntryKind};
use crate::remote::{Chat, Message};

impl Conversation {
    pub(in crate::application) fn handoff(&mut self, chat: &Chat) {
        self.handoff_prefix = (!chat.messages.is_empty()).then(|| chat.prompt(""));
        self.entries
            .extend(chat.messages.iter().enumerate().map(|(i, message)| {
                let (kind, text) = match message {
                    Message::User(text) => (EntryKind::User, text),
                    Message::Assistant(text) => (EntryKind::Assistant, text),
                };
                Entry::new(format!("handoff-{i}"), kind, text.clone(), true)
            }));
    }

    /// Keep the captured chat at the beginning when the peer rehydrates or paginates.
    /// Normalize only the envelope built for this session, before matching provider echoes.
    pub(super) fn handoff_rows(&self, live: &mut Vec<Entry>, history: &mut [Entry]) -> Vec<Entry> {
        let Some(prefix) = self.handoff_prefix.as_deref() else {
            return Vec::new();
        };
        for entry in history {
            if entry.kind == EntryKind::User && entry.text.starts_with(prefix) {
                entry.text.drain(..prefix.len());
            }
        }
        let count = live.partition_point(|entry| entry.id.starts_with("handoff-"));
        live.drain(..count).collect()
    }
}
