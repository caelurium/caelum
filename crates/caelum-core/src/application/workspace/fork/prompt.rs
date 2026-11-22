use crate::application::{presentation, split_context, Attachment, AttachmentKind, Provider};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::Value;

#[derive(Debug, Clone)]
pub(super) struct Draft {
    pub text: String,
    pub attachments: Vec<Attachment>,
}

/// Reuse immutable provider bytes, never open a path from old conversation history.
pub(super) fn recover(provider: Provider, text: &str, user: &Value) -> Result<Draft, String> {
    let (text, mut attachments) = split_context(text);
    let parts = match provider {
        Provider::Codex => &user["content"],
        Provider::OpenCode => &user["files"],
        Provider::Antigravity => &Value::Null,
    };
    for part in parts.as_array().into_iter().flatten() {
        if provider == Provider::Codex && part["type"] == "text" {
            continue;
        }
        const MISSING: &str = "This prompt has a provider attachment without a reusable snapshot. Its original context cannot be resent.";
        let attachment =
            if let Some(media) = presentation::media(&Value::Array(vec![part.clone()])).pop() {
                let stored = provider == Provider::OpenCode
                    && media
                        .source
                        .strip_prefix("opencode-media:")
                        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
                if !stored
                    && (!media.source.starts_with("data:image/")
                        || media.source.len() > 14 * 1024 * 1024)
                {
                    return Err(MISSING.into());
                }
                let mut a = Attachment::new(media.name, String::new(), None, false);
                a.kind = AttachmentKind::Image;
                a.content = media.source;
                a
            } else if provider == Provider::OpenCode
                && part["mime"]
                    .as_str()
                    .is_some_and(|m| m.starts_with("text/"))
            {
                let data = part["data"]
                    .as_str()
                    .filter(|s| s.len() <= 88 * 1024)
                    .ok_or(MISSING)?;
                let raw = STANDARD.decode(data).map_err(|_| MISSING)?;
                if raw.len() > 64 * 1024 {
                    return Err(MISSING.into());
                }
                Attachment::new(
                    part["name"].as_str().unwrap_or("Attachment").into(),
                    String::from_utf8(raw).map_err(|_| MISSING)?,
                    None,
                    false,
                )
            } else {
                return Err(MISSING.into());
            };
        attachments.push(attachment);
    }
    if attachments.len() > 16
        || ["agents", "skills"]
            .iter()
            .any(|k| user[*k].as_array().is_some_and(|a| !a.is_empty()))
    {
        return Err(
            "This prompt includes provider context that cannot be faithfully replayed.".into(),
        );
    }
    Ok(Draft {
        text: if text.contains("<system-reminder>") {
            presentation::visible_text(text)
        } else {
            text.into()
        },
        attachments,
    })
}
