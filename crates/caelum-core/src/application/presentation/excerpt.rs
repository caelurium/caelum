use super::{markdown_parts, Entry, EntryKind};

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

pub(super) fn message_excerpt(value: &str) -> Option<String> {
    let head = &value[..value.floor_char_boundary(value.len().min(4096))];
    let head = [
        "\n\n<caelum-file-context>",
        "\n\n<caelum-image-context>",
        "\n\n<caelum-pull-request-context>",
    ]
    .into_iter()
    .fold(head, |text, tag| {
        text.split_once(tag).map_or(text, |(before, _)| before)
    });
    let head = match head.rsplit_once("![") {
        Some((text, image)) if !image.contains(')') => text,
        _ => head,
    };
    let text = if head.contains(['<', '!']) {
        excerpt(&markdown_parts(head, true).0)
    } else {
        excerpt(head)
    };
    (!text.is_empty()).then_some(text)
}

/// A bounded, single-line preview of the latest visible exchange; tool output,
/// attachments and provider reminders do not become conversation summaries.
pub fn latest_message(entries: &[Entry]) -> Option<(EntryKind, String)> {
    entries.iter().rev().find_map(|entry| {
        let kind = entry.kind();
        if !matches!(kind, EntryKind::User | EntryKind::Assistant) {
            return None;
        }
        message_excerpt(&entry.text).map(|shown| (kind, shown))
    })
}
