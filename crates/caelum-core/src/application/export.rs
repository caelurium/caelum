//! Export provider history, independently of the mounted transcript or its pagination.
use super::{presentation, split_context, Conversation, Provider, SessionKey};
use caelum_protocol::SessionHistoryPage;
use serde::Serialize;
use serde_json::Value;
use std::{collections::HashSet, io::Write, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
    pub fn scope(self) -> &'static str {
        match self {
            Self::Markdown => {
                "Transcript, tool output and attachment references from all history pages"
            }
            Self::Json => {
                "All provider history records, including tool output and stored attachment data"
            }
        }
    }
}

pub(super) fn save(
    key: &SessionKey,
    title: &str,
    format: ExportFormat,
    path: &Path,
    mut read: impl FnMut(Option<&str>) -> Result<SessionHistoryPage, String>,
) -> Result<(), String> {
    let mut pages = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor = None;
    let mut bytes = 0;
    loop {
        let page = read(cursor.as_deref())?;
        if page.is_working == Some(true) {
            return Err("Wait for the conversation to finish before exporting.".into());
        }
        if page.data.is_empty() && page.next_cursor.is_some() {
            return Err(
                "Provider returned an incomplete history page; no export was written.".into(),
            );
        }
        for record in &page.data {
            let id = record["id"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("History record has no ID")?;
            if !ids.insert(id.to_owned())
                || (key.scope.provider == Provider::Codex
                    && (!record["items"].is_array()
                        || (!record["itemsView"].is_null() && record["itemsView"] != "full")))
            {
                return Err("Incomplete or overlapping history; no export was written.".into());
            }
            bytes += serde_json::to_vec(record).map_err(|e| e.to_string())?.len();
            if bytes > 128 * 1024 * 1024 {
                return Err(
                    "History exceeds the 128 MiB export limit; no partial export was written."
                        .into(),
                );
            }
        }
        pages.push(page.data);
        cursor = page.next_cursor;
        match cursor.as_ref() {
            Some(c) if !c.is_empty() && cursors.insert(c.clone()) => {}
            Some(_) => {
                return Err("History cursor repeated or was empty; no export was written.".into())
            }
            None => break,
        }
    }
    if pages.len() > 1 {
        let head = read(None)?;
        if head.is_working == Some(true) || head.data != pages[0] {
            return Err(
                "History changed during export. Try again when the session is idle.".into(),
            );
        }
    }
    // Do not truncate an existing destination on provider, format, or write failure.
    let write = || -> std::io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        let mut out = std::io::BufWriter::new(file.as_file_mut());
        let records = pages.into_iter().rev().flatten();
        match format {
            ExportFormat::Json => {
                #[derive(Serialize)]
                struct Document<'a> {
                    version: u8,
                    session: &'a SessionKey,
                    title: &'a str,
                    scope: &'static str,
                    records: Vec<Value>,
                }
                serde_json::to_writer_pretty(
                    &mut out,
                    &Document {
                        version: 1,
                        session: key,
                        title,
                        scope: format.scope(),
                        records: records.collect(),
                    },
                )?;
                writeln!(out)?;
            }
            ExportFormat::Markdown => {
                writeln!(
                    out,
                    "# {title}\n\n{} · {}\n\n{}\n",
                    key.scope.provider.as_str(),
                    key.id,
                    format.scope()
                )?;
                for record in records {
                    markdown_record(&mut out, key.scope.provider, &record)?;
                }
            }
        }
        out.flush()?;
        drop(out);
        file.persist(path)?;
        Ok(())
    };
    write().map_err(|e| format!("Could not save export: {e}"))
}

#[cfg(test)]
mod tests;

fn markdown_record(
    out: &mut impl Write,
    provider: Provider,
    record: &Value,
) -> std::io::Result<()> {
    let mut conversation = Conversation::default();
    conversation.history(provider, std::slice::from_ref(record), false);
    let user_files = (provider == Provider::OpenCode && record["type"] == "user")
        .then(|| record["files"].as_array())
        .flatten();
    for entry in conversation.entries {
        if !presentation::entry_visible(&entry) && user_files.is_none_or(Vec::is_empty) {
            continue;
        }
        writeln!(
            out,
            "## {:?}{}\n",
            entry.kind,
            if entry.name.is_empty() {
                String::new()
            } else {
                format!(": {}", entry.name)
            }
        )?;
        writeln!(out, "{}\n", presentation::entry_text(&entry))?;
        for (label, value) in [
            ("Input", &entry.input),
            ("Output", &entry.output),
            ("Error", &entry.error),
        ] {
            if !value.is_empty() {
                writeln!(out, "{label}:\n")?;
                for line in value.lines() {
                    writeln!(out, "    {line}")?;
                }
                writeln!(out)?;
            }
        }
        for a in split_context(&entry.text).1 {
            writeln!(out, "Attachment: {}\n", a.label())?;
        }
        for m in entry.media {
            let source = if m.source.starts_with("data:") {
                "embedded image"
            } else {
                &m.source
            };
            writeln!(out, "Attachment: {} ({source})\n", m.name)?;
        }
        if let Some(files) = user_files {
            for f in files
                .iter()
                .filter(|f| !f["mime"].as_str().unwrap_or("").starts_with("image/"))
            {
                writeln!(
                    out,
                    "Attachment: {}\n",
                    f["name"].as_str().unwrap_or("File")
                )?;
            }
        }
    }
    Ok(())
}
