use super::{entry_text, Entry, EntryKind};

pub fn excerpt(value: &str) -> String {
    let mut chars = value
        .split_whitespace()
        .flat_map(|word| word.chars().chain([' ']));
    let mut snippet: String = chars.by_ref().take(180).collect();
    snippet.truncate(snippet.trim_end().len());
    if chars.any(|c| c != ' ') {
        snippet.push('…');
    }
    snippet
}

/// A bounded, single-line preview of the latest visible exchange; tool output,
/// attachments and provider reminders do not become conversation summaries.
pub fn latest_message(entries: &[Entry]) -> Option<(EntryKind, String)> {
    entries.iter().rev().find_map(|entry| {
        let kind = entry.kind();
        if !matches!(kind, EntryKind::User | EntryKind::Assistant) {
            return None;
        }
        let shown = if !entry.text.contains(['<', '!']) {
            excerpt(&entry.text)
        } else {
            excerpt(&entry_text(entry))
        };
        (!shown.is_empty()).then_some((kind, shown))
    })
}
