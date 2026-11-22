use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};

use caelum_protocol::{AntigravityEvent, QuestionPrompt};
use serde_json::{json, Value};

use super::{
    lock, progress::SessionProgressTracker, transport, EventSubscribers, PendingRequests,
    PROVIDER_NAME,
};
use crate::questions::{Answer, QuestionBroker};

pub(super) struct QuestionReplyContext {
    pub project_path: String,
    pub session_id: String,
    pub writer: Arc<Mutex<std::process::ChildStdin>>,
    pub pending: PendingRequests,
    pub next_id: Arc<AtomicU64>,
    pub alive: Arc<AtomicBool>,
    pub progress: Arc<Mutex<SessionProgressTracker>>,
    pub subscribers: EventSubscribers,
}

pub(super) fn is_ask_question(update: &Value) -> bool {
    update["name"].as_str() == Some("ask_question")
}

pub(super) fn register_from_update(
    questions: &QuestionBroker,
    context: QuestionReplyContext,
    update: &Value,
) {
    let Some((prompt, titles)) =
        parse_question_prompt(&context.project_path, &context.session_id, update)
    else {
        return;
    };
    let fields = prompt.fields.clone();
    let reply = Arc::new(move |answer: Option<Answer>| {
        let Some(answer) = answer else {
            return Ok(());
        };
        if !context.alive.load(Ordering::Acquire) {
            return Err("Antigravity disconnected before the answer could be sent".into());
        }
        let text = format_answer(&titles, &fields, &answer);
        if text.trim().is_empty() {
            return Err("Answer before submitting".into());
        }
        lock(&context.progress).on_prompt_started(&context.session_id);
        transport::publish_event(
            &context.subscribers,
            AntigravityEvent {
                project_path: context.project_path.clone(),
                session_id: Some(context.session_id.clone()),
                method: "antigravity/turnStarted".into(),
                params: json!({ "sessionId": context.session_id }),
            },
        );
        if let Err(error) = transport::send_prompt_parts(
            &context.writer,
            &context.pending,
            &context.next_id,
            &context.alive,
            &context.session_id,
            json!({
                "sessionId": context.session_id,
                "prompt": [{ "type": "text", "text": text }],
            }),
        ) {
            lock(&context.progress).on_prompt_send_failed(&context.session_id);
            return Err(error.to_string());
        }
        Ok(())
    });
    let _ = questions.insert(prompt, reply);
}

pub(super) fn parse_question_prompt(
    project_path: &str,
    session_id: &str,
    update: &Value,
) -> Option<(QuestionPrompt, Vec<String>)> {
    if !is_ask_question(update) {
        return None;
    }
    let request_id = update["toolCallId"].as_str().filter(|id| !id.is_empty())?;
    let input = if !update["rawInput"].is_null() {
        &update["rawInput"]
    } else {
        &update["args"]
    };
    let raw_questions = input["questions"].as_array()?;
    let mut fields = Vec::with_capacity(raw_questions.len());
    let mut titles = Vec::with_capacity(raw_questions.len());

    for (index, item) in raw_questions.iter().enumerate() {
        let question = item["question"].as_str()?.trim();
        if question.is_empty() {
            continue;
        }
        let multi = item["is_multi_select"].as_bool().unwrap_or(false);
        let options: Vec<Value> = item["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| {
                let label = option.as_str()?.trim();
                (!label.is_empty()).then(|| json!({ "value": label, "label": label }))
            })
            .collect();
        fields.push(json!({
            "key": format!("q{index}"),
            "type": if multi { "multiselect" } else { "string" },
            "title": question,
            "required": true,
            "custom": true,
            "options": options,
        }));
        titles.push(question.to_owned());
    }

    if fields.is_empty() {
        return None;
    }

    Some((
        QuestionPrompt {
            provider: PROVIDER_NAME.into(),
            project_path: project_path.to_owned(),
            session_id: session_id.to_owned(),
            request_id: request_id.to_owned(),
            title: if fields.len() == 1 {
                "Question".into()
            } else {
                "Questions".into()
            },
            fields,
        },
        titles,
    ))
}

pub(super) fn format_answer(titles: &[String], fields: &[Value], answer: &Answer) -> String {
    let mut lines = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let Some(key) = field["key"].as_str() else {
            continue;
        };
        let Some(value) = answer.get(key) else {
            continue;
        };
        let formatted = match value {
            Value::String(text) => text.trim().to_owned(),
            Value::Array(items) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", "),
            other => other.to_string(),
        };
        if formatted.is_empty() {
            continue;
        }
        if fields.len() == 1 {
            lines.push(formatted);
        } else {
            let title = titles
                .get(index)
                .map(String::as_str)
                .or_else(|| field["title"].as_str())
                .unwrap_or(key);
            lines.push(format!("{title}: {formatted}"));
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{format_answer, parse_question_prompt};

    #[test]
    fn parses_single_and_multi_select_questions_with_custom_input_enabled() {
        let update = json!({
            "toolCallId": "call_1",
            "name": "ask_question",
            "status": "pending",
            "rawInput": {
                "questions": [
                    {
                        "question": "Which color do you prefer?",
                        "options": ["Red", "Blue"],
                        "is_multi_select": false
                    },
                    {
                        "question": "Which checks should run?",
                        "options": ["Lint", "Tests"],
                        "is_multi_select": true
                    }
                ]
            }
        });
        let (prompt, titles) = parse_question_prompt("/project", "s1", &update).unwrap();
        assert_eq!(prompt.provider, "antigravity");
        assert_eq!(prompt.request_id, "call_1");
        assert_eq!(prompt.title, "Questions");
        assert_eq!(prompt.fields.len(), 2);
        assert_eq!(prompt.fields[0]["type"], "string");
        assert_eq!(prompt.fields[0]["custom"], true);
        assert_eq!(prompt.fields[1]["type"], "multiselect");
        assert_eq!(prompt.fields[1]["custom"], true);

        let answer = json!({
            "q0": "Blue",
            "q1": ["Lint", "Tests"]
        })
        .as_object()
        .unwrap()
        .clone();
        assert_eq!(
            format_answer(&titles, &prompt.fields, &answer),
            "Which color do you prefer?: Blue\nWhich checks should run?: Lint, Tests"
        );
    }

    #[test]
    fn formats_single_question_answer_without_prefix() {
        let update = json!({
            "toolCallId": "call_2",
            "name": "ask_question",
            "status": "pending",
            "args": {
                "questions": [
                    {
                        "question": "Continue?",
                        "options": ["Yes", "No"],
                        "is_multi_select": false
                    }
                ]
            }
        });
        let (prompt, titles) = parse_question_prompt("/project", "s1", &update).unwrap();
        assert_eq!(prompt.title, "Question");
        let answer = json!({ "q0": "Yes" }).as_object().unwrap().clone();
        assert_eq!(format_answer(&titles, &prompt.fields, &answer), "Yes");
    }
}
