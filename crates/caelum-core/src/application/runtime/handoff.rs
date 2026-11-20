use super::{Provider, ProviderRuntime, SessionKey};
use crate::{
    application::{presentation, Conversation, EntryKind},
    remote::{Chat, Message},
};
use caelum_protocol::SessionHistoryPage;
use std::collections::HashSet;

impl ProviderRuntime {
    pub(super) fn handoff_chat(&self, key: &SessionKey) -> Result<Chat, String> {
        collect(key.scope.provider, |cursor| self.history_page(key, cursor))
    }
}

// Read independently of the mounted transcript. Both providers return newest
// pages first with each page in chronological order. Keep only one raw page.
fn collect(
    provider: Provider,
    mut read: impl FnMut(Option<&str>) -> Result<SessionHistoryPage, String>,
) -> Result<Chat, String> {
    const MAX_BYTES: usize = 64 * 1024;
    let mut chat = Chat::default();
    let mut bytes = 0;
    let mut entries = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor = None;
    'pages: loop {
        let page = read(cursor.as_deref())?;
        if page.is_working == Some(true) {
            return Err("Finish the current turn before continuing on a VM.".into());
        }
        let mut conversation = Conversation::default();
        conversation.history(provider, &page.data, false);
        for entry in conversation.entries.into_iter().rev() {
            let message = match entry.kind {
                EntryKind::User => Message::User,
                EntryKind::Assistant => Message::Assistant,
                _ => continue,
            };
            let text = presentation::entry_text(&entry);
            if text.trim().is_empty() || !entries.insert(entry.id) {
                continue;
            }
            if chat.messages.len() == 200 || text.len() > MAX_BYTES - bytes {
                if chat.messages.is_empty() {
                    // Keep a useful suffix even for a single oversized response.
                    let start = text.ceil_char_boundary(text.len() - MAX_BYTES);
                    chat.messages.push(message(text[start..].into()));
                }
                chat.clipped = true;
                break 'pages;
            }
            bytes += text.len();
            chat.messages.push(message(text));
        }
        cursor = page.next_cursor;
        let Some(next) = &cursor else { break };
        if next.is_empty() || !cursors.insert(next.clone()) {
            return Err("The provider returned a repeated or empty history cursor.".into());
        }
        if cursors.len() >= 128 {
            chat.clipped = true;
            break;
        }
    }
    chat.messages.reverse();
    Ok(chat)
}

#[cfg(test)]
mod tests;
