//! Current provider failures are recoverable notices, not transcript messages.
use serde_json::Value;

#[derive(Debug)]
pub struct Notice {
    pub generation: u64,
    pub message: String,
    pub details: String,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Clear,
    Failed(Notice),
}

#[derive(Debug, Default)]
pub struct State {
    phase: Phase,
    generation: u64,
    observed_at: u64,
}

impl State {
    pub fn notice(&self) -> Option<&Notice> {
        match &self.phase {
            Phase::Failed(notice) => Some(notice),
            Phase::Clear => None,
        }
    }

    pub fn dismiss(&mut self, generation: u64) {
        if self.notice().is_some_and(|n| n.generation == generation) {
            self.clear();
        }
    }

    fn clear(&mut self) {
        self.phase = Phase::Clear;
    }

    pub(super) fn begin(&mut self, at: u64) {
        self.observed_at = self.observed_at.max(at);
        self.clear();
    }

    pub(in crate::application) fn is_stale(&self, at: Option<u64>) -> bool {
        at.is_some_and(|at| at < self.observed_at)
    }

    pub(super) fn event(&mut self, method: &str, data: &Value) -> bool {
        let failed = method == "session.execution.failed";
        let recovered = matches!(
            method,
            "session.execution.started"
                | "session.step.started"
                | "session.retry.scheduled"
                | "session.text.delta"
                | "session.reasoning.delta"
                | "session.execution.succeeded"
                | "session.execution.interrupted"
        ) || (method == "session.status"
            && matches!(data["status"]["type"].as_str(), Some("busy" | "retry")));
        if !failed && !recovered {
            return true;
        }
        let at = data["_created"].as_u64().unwrap_or(self.observed_at);
        if at < self.observed_at {
            return !failed;
        }
        self.observed_at = at;
        if failed {
            self.fail(&data["error"]);
        } else {
            self.clear();
        }
        true
    }

    pub(in crate::application) fn history(&mut self, messages: &[Value], working: bool) {
        if working {
            self.clear();
            return;
        }
        let Some(message) = messages.iter().rev().find(|message| {
            matches!(
                message["type"].as_str(),
                Some("user" | "assistant" | "idle")
            )
        }) else {
            return;
        };
        let Some(at) = message["time"]["created"].as_u64() else {
            return;
        };
        if at <= self.observed_at {
            return;
        }
        self.observed_at = at;
        if message["type"] == "idle" && message["outcome"] == "failed" {
            self.fail(&message["error"]);
        } else {
            self.clear();
        }
    }

    fn fail(&mut self, error: &Value) {
        self.generation += 1;
        let details = super::display(error);
        let message = if error["response"]["body"].as_str().is_some_and(|body| {
            serde_json::from_str::<Value>(body).is_ok_and(|response| {
                response["error"]["code"] == "subscription_sharing_unsupported_capability"
            })
        }) {
            "ChatGPT token sharing rejected part of this request. Start a new session with the model and effort selected before sending. If it still fails, check the OpenCode connection or use an API-key connection.".into()
        } else {
            error["message"]
                .as_str()
                .or(error["error"]["message"].as_str())
                .or(error.as_str())
                .filter(|message| !message.trim().is_empty())
                .unwrap_or("The provider could not finish this response.")
                .to_owned()
        };
        self.phase = Phase::Failed(Notice {
            generation: self.generation,
            message,
            details,
        });
    }
}
