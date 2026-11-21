use crate::remote::{Line, Run, Speaker, Thread};
use serde_json::{Map, Value};

pub(super) fn append(
    lines: &mut Vec<Line>,
    data: &Map<String, Value>,
    event: Option<&str>,
    run: &Run,
) {
    let Some(content) = data.get("content") else {
        return;
    };
    let streaming = data.get("is_streaming").and_then(Value::as_bool) == Some(true);
    let key = event.map(|id| format!("{}:response:{id}", run.id));
    let bounded =
        |s: &str| s[..s.floor_char_boundary(s.len().min(Thread::MAX_MESSAGE_BYTES))].to_owned();
    if let Some(s) = content.as_str() {
        if !s.is_empty() {
            lines.push(Line {
                key,
                partial: streaming,
                ..Line::new(run.provider.into(), bounded(s))
            });
        }
        return;
    }
    let Some(parts) = content.as_array() else {
        return;
    };
    for (i, item) in parts.iter().take(64).enumerate() {
        let (speaker, text, partial) = match item["type"].as_str() {
            Some("text" | "output_text") => (
                run.provider.into(),
                bounded(item["text"].as_str().unwrap_or("")),
                streaming,
            ),
            // If a harness forwards a public Responses-style reasoning item, only
            // the typed summary is for display. Never fall back to its raw content.
            Some("reasoning") => {
                let mut text = String::new();
                for part in item["summary"].as_array().into_iter().flatten().take(64) {
                    if part["type"] != "summary_text" {
                        continue;
                    }
                    let Some(s) = part["text"].as_str().filter(|s| !s.is_empty()) else {
                        continue;
                    };
                    if !text.is_empty() {
                        text.push_str("\n\n");
                    }
                    let room = Thread::MAX_MESSAGE_BYTES.saturating_sub(text.len());
                    text.push_str(&s[..s.floor_char_boundary(s.len().min(room))]);
                    if text.len() + 2 >= Thread::MAX_MESSAGE_BYTES {
                        break;
                    }
                }
                let partial = match item["status"].as_str() {
                    Some("in_progress") => true,
                    Some("completed" | "incomplete") => false,
                    _ => streaming,
                };
                (Speaker::ReasoningSummary, text, partial)
            }
            Some("summary_text") => (
                Speaker::ReasoningSummary,
                bounded(item["text"].as_str().unwrap_or("")),
                streaming,
            ),
            _ => continue,
        };
        if text.is_empty() && speaker != Speaker::ReasoningSummary {
            continue;
        }
        let part_key = item["id"]
            .as_str()
            .filter(|id| super::event_id(id))
            .map(|id| format!("{}:part:{id}", run.id))
            .or_else(|| event.map(|id| format!("{}:content-part:{i}:{id}", run.id)));
        lines.push(Line {
            key: part_key,
            partial,
            ..Line::new(speaker, text)
        });
    }
}
