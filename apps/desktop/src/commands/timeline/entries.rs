use caelum_core::application::{Entry, EntryKind};

pub(super) const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Copy)]
pub(super) struct TurnSummary {
    pub(super) ordinal: usize,
    pub(super) position: usize,
    pub(super) target_position: usize,
    pub(super) tools: usize,
    pub(super) attachments: usize,
    pub(super) failed: bool,
    pub(super) running: bool,
    pub(super) compacted: bool,
}

fn summarize_turns(entries: &[Entry]) -> Vec<TurnSummary> {
    let positions: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.kind() == EntryKind::User
                && caelum_core::application::presentation::entry_visible(entry)
        })
        .map(|(i, _)| i)
        .collect();

    positions
        .iter()
        .enumerate()
        .map(|(idx, &start)| {
            let end = positions.get(idx + 1).copied().unwrap_or(entries.len());
            let mut tools = 0;
            let mut failed = false;
            let mut running = false;
            let mut compacted = false;
            for entry in &entries[start + 1..end] {
                if !entry.complete() {
                    running = true;
                }
                if !entry.error.is_empty() || matches!(entry.status(), "error" | "failed") {
                    failed = true;
                }
                match entry.kind() {
                    EntryKind::Tool => tools += 1,
                    EntryKind::Compaction => compacted = true,
                    _ => {}
                }
            }
            TurnSummary {
                ordinal: idx + 1,
                position: start,
                target_position: start,
                tools,
                attachments: entries[start].media.len(),
                failed,
                running,
                compacted,
            }
        })
        .collect()
}

pub(super) fn matching_turns(
    session: &caelum_core::application::Session,
    query: &str,
    bookmarked_only: bool,
) -> Vec<TurnSummary> {
    let entries = &session.conversation.entries;
    let turns = summarize_turns(entries);
    let mut matches = Vec::new();

    for (index, turn) in turns.iter().enumerate() {
        let end = turns
            .get(index + 1)
            .map_or(entries.len(), |next| next.position);
        let mut add_match = |position: usize| {
            let entry = &entries[position];
            let eligible = if bookmarked_only {
                matches!(entry.kind(), EntryKind::User | EntryKind::Assistant)
                    && session.is_bookmarked(&entry.id)
            } else {
                true
            };
            if eligible
                && caelum_core::application::presentation::entry_visible(entry)
                && caelum_core::application::presentation::entry_text(entry)
                    .to_lowercase()
                    .contains(query)
            {
                matches.push(TurnSummary {
                    target_position: position,
                    ..*turn
                });
            }
        };

        if bookmarked_only {
            for position in turn.position..end {
                add_match(position);
            }
        } else {
            add_match(turn.position);
        }
    }

    matches
}

pub(super) fn day_bucket(entry: &Entry) -> Option<u64> {
    entry.started_at.map(|at| at / DAY_MS)
}
