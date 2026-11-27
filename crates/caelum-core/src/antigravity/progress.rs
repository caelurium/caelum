use std::{
    collections::{BTreeSet, HashMap, HashSet},
    time::Duration,
};

use serde_json::Value;

pub(super) const REACTIVE_SETTLE_DELAY: Duration = Duration::from_millis(750);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProgressEffect {
    TurnYielded {
        session_id: String,
    },
    TurnCompleted {
        session_id: String,
        stop_reason: String,
        error: Option<String>,
    },
    ScheduleSettle {
        session_id: String,
        generation: u64,
    },
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    Prompting {
        request: u64,
        tools: BTreeSet<String>,
    },
    Background(ActiveTools),
    Reactive,
    Settling(u64),
}

#[derive(Debug)]
struct ActiveTools {
    first: String,
    rest: BTreeSet<String>,
}

impl ActiveTools {
    fn from_set(mut tools: BTreeSet<String>) -> Option<Self> {
        Some(Self {
            first: tools.pop_first()?,
            rest: tools,
        })
    }

    fn into_set(mut self) -> BTreeSet<String> {
        self.rest.insert(self.first);
        self.rest
    }

    fn insert(&mut self, tool: &str) {
        if tool != self.first {
            self.rest.insert(tool.to_owned());
        }
    }

    fn finish(&mut self, tool: &str) -> bool {
        if tool != self.first {
            self.rest.remove(tool);
            return false;
        }
        if let Some(next) = self.rest.pop_first() {
            self.first = next;
            false
        } else {
            true
        }
    }
}

/// Tracks per-session Antigravity turn lifecycle across foreground prompts,
/// background tools that outlive `session/prompt`, and reactive follow-up turns.
#[derive(Debug, Default)]
pub(super) struct SessionProgressTracker {
    sessions: HashMap<String, Phase>,
    next_generation: u64,
}

impl SessionProgressTracker {
    pub(super) fn is_working(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .is_some_and(|phase| !matches!(phase, Phase::Idle))
    }

    pub(super) fn working_sessions(&self) -> HashSet<String> {
        self.sessions
            .iter()
            .filter(|(_, phase)| !matches!(phase, Phase::Idle))
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(super) fn on_prompt_started(&mut self, session_id: &str, request: u64) {
        let phase = self.sessions.entry(session_id.to_owned()).or_default();
        let tools = match std::mem::take(phase) {
            Phase::Prompting { tools, .. } => tools,
            Phase::Background(tools) => tools.into_set(),
            Phase::Idle | Phase::Reactive | Phase::Settling(_) => BTreeSet::new(),
        };
        *phase = Phase::Prompting { request, tools };
    }

    pub(super) fn on_prompt_send_failed(&mut self, session_id: &str, request: u64) {
        if let Some(phase) = self.sessions.get_mut(session_id) {
            if matches!(phase, Phase::Prompting { request: current, .. } if *current == request) {
                promote_background(phase);
            }
        }
    }

    pub(super) fn on_prompt_replied(
        &mut self,
        session_id: &str,
        request: u64,
        result: Result<Value, String>,
    ) -> Option<ProgressEffect> {
        let phase = self.sessions.get_mut(session_id)?;
        if !matches!(phase, Phase::Prompting { request: current, .. } if *current == request) {
            return None;
        }
        let (stop_reason, error) = match result {
            Err(error) => ("error".into(), Some(error)),
            Ok(reply) => {
                let stop_reason = reply["stopReason"]
                    .as_str()
                    .unwrap_or("end_turn")
                    .to_owned();
                if stop_reason == "end_turn" {
                    promote_background(phase);
                    if matches!(phase, Phase::Background(_)) {
                        return Some(ProgressEffect::TurnYielded {
                            session_id: session_id.to_owned(),
                        });
                    }
                }
                (stop_reason, None)
            }
        };
        *phase = Phase::Idle;
        Some(ProgressEffect::TurnCompleted {
            session_id: session_id.to_owned(),
            stop_reason,
            error,
        })
    }

    pub(super) fn on_session_update(
        &mut self,
        session_id: &str,
        kind: &str,
        update: &Value,
    ) -> Option<ProgressEffect> {
        match kind {
            "tool_call" | "tool_call_update" => {
                if super::questions::is_ask_question(update) {
                    return None;
                }
                let tool = update["toolCallId"].as_str().filter(|s| !s.is_empty())?;
                let active = is_tool_update_active(kind, update["status"].as_str());
                let phase = self.sessions.entry(session_id.to_owned()).or_default();
                match phase {
                    Phase::Prompting { tools, .. } => {
                        if active {
                            tools.insert(tool.to_owned());
                        } else {
                            tools.remove(tool);
                        }
                    }
                    Phase::Background(tools) => {
                        if active {
                            tools.insert(tool);
                        } else if tools.finish(tool) {
                            *phase = Phase::Reactive;
                        }
                    }
                    Phase::Idle | Phase::Reactive | Phase::Settling(_) if active => {
                        *phase = Phase::Background(ActiveTools {
                            first: tool.to_owned(),
                            rest: BTreeSet::new(),
                        });
                    }
                    _ => {}
                }
                None
            }
            "agent_thought_chunk" | "agent_message_chunk" | "plan" => {
                let phase = self.sessions.entry(session_id.to_owned()).or_default();
                if matches!(phase, Phase::Idle | Phase::Settling(_)) {
                    *phase = Phase::Reactive;
                }
                None
            }
            "usage_update" => {
                let phase = self.sessions.get_mut(session_id)?;
                match phase {
                    Phase::Background(_) => Some(ProgressEffect::TurnYielded {
                        session_id: session_id.to_owned(),
                    }),
                    Phase::Reactive | Phase::Settling(_) => {
                        self.next_generation = self.next_generation.wrapping_add(1).max(1);
                        let generation = self.next_generation;
                        *phase = Phase::Settling(generation);
                        Some(ProgressEffect::ScheduleSettle {
                            session_id: session_id.to_owned(),
                            generation,
                        })
                    }
                    Phase::Idle | Phase::Prompting { .. } => None,
                }
            }
            _ => None,
        }
    }

    pub(super) fn on_settle_timer(
        &mut self,
        session_id: &str,
        generation: u64,
    ) -> Option<ProgressEffect> {
        let phase = self.sessions.get_mut(session_id)?;
        if !matches!(*phase, Phase::Settling(current) if current == generation) {
            return None;
        }
        *phase = Phase::Idle;
        Some(ProgressEffect::TurnCompleted {
            session_id: session_id.to_owned(),
            stop_reason: "end_turn".into(),
            error: None,
        })
    }

    pub(super) fn on_cancel(&mut self, session_id: &str) -> Option<ProgressEffect> {
        let phase = self.sessions.get_mut(session_id)?;
        if matches!(phase, Phase::Idle | Phase::Prompting { .. }) {
            return None;
        }
        *phase = Phase::Idle;
        Some(ProgressEffect::TurnCompleted {
            session_id: session_id.to_owned(),
            stop_reason: "cancelled".into(),
            error: None,
        })
    }

    pub(super) fn on_replayed_history(&mut self, session_id: &str, active_tools: Vec<String>) {
        let Some(tools) = ActiveTools::from_set(active_tools.into_iter().collect()) else {
            return;
        };
        let phase = self.sessions.entry(session_id.to_owned()).or_default();
        if matches!(phase, Phase::Idle) {
            *phase = Phase::Background(tools);
        }
    }

    pub(super) fn on_disconnected(&mut self) -> Vec<ProgressEffect> {
        self.sessions
            .drain()
            .filter(|(_, phase)| {
                matches!(
                    phase,
                    Phase::Background(_) | Phase::Reactive | Phase::Settling(_)
                )
            })
            .map(|(session_id, _)| ProgressEffect::TurnCompleted {
                session_id,
                stop_reason: "disconnected".into(),
                error: Some("Antigravity ACP connection closed".into()),
            })
            .collect()
    }
}

fn promote_background(phase: &mut Phase) {
    if let Phase::Prompting { tools, .. } = phase {
        *phase =
            ActiveTools::from_set(std::mem::take(tools)).map_or(Phase::Idle, Phase::Background);
    }
}

pub(super) fn is_active_tool_status(status: Option<&str>) -> bool {
    matches!(status, Some("pending" | "in_progress" | "running"))
}

fn is_tool_update_active(kind: &str, status: Option<&str>) -> bool {
    match status {
        Some(s) => is_active_tool_status(Some(s)),
        None => kind == "tool_call",
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ProgressEffect, SessionProgressTracker};

    #[test]
    fn a_superseded_prompt_cannot_complete_the_new_turn() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1", 1);
        tracker.on_prompt_started("s1", 2);
        tracker.on_prompt_send_failed("s1", 1);
        assert!(tracker.on_prompt_replied("s1", 1, Ok(json!({}))).is_none());
        assert!(tracker.is_working("s1"));
        assert!(matches!(
            tracker.on_prompt_replied("s1", 2, Ok(json!({}))),
            Some(ProgressEffect::TurnCompleted { .. })
        ));
        assert!(!tracker.is_working("s1"));
    }

    #[test]
    fn synchronous_turn_completes_immediately_on_prompt_reply() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1", 1);
        assert!(tracker.is_working("s1"));

        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "t1", "status": "in_progress" }),
        );
        tracker.on_session_update(
            "s1",
            "tool_call_update",
            &json!({ "toolCallId": "t1", "status": "completed" }),
        );
        let effect = tracker
            .on_prompt_replied("s1", 1, Ok(json!({ "stopReason": "end_turn" })))
            .unwrap();
        assert_eq!(
            effect,
            ProgressEffect::TurnCompleted {
                session_id: "s1".into(),
                stop_reason: "end_turn".into(),
                error: None,
            }
        );
        assert!(!tracker.is_working("s1"));
    }

    #[test]
    fn background_tool_keeps_session_working_until_reactive_turn_settles() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1", 1);
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );

        let effect = tracker
            .on_prompt_replied("s1", 1, Ok(json!({ "stopReason": "end_turn" })))
            .unwrap();
        assert_eq!(
            effect,
            ProgressEffect::TurnYielded {
                session_id: "s1".into()
            }
        );
        assert!(tracker.is_working("s1"));

        // Background tool completes -> reactive wakeup begins.
        assert_eq!(
            tracker.on_session_update(
                "s1",
                "tool_call_update",
                &json!({ "toolCallId": "bg1", "status": "completed" }),
            ),
            None
        );
        assert!(tracker.is_working("s1"));

        // Reactive step 1: emits usage_update right before calling another tool.
        let Some(ProgressEffect::ScheduleSettle { generation: g1, .. }) =
            tracker.on_session_update("s1", "usage_update", &json!({ "used": 100 }))
        else {
            panic!("expected settle schedule");
        };
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "t2", "status": "in_progress" }),
        );
        // Stale timer g1 must not settle the session.
        assert_eq!(tracker.on_settle_timer("s1", g1), None);
        assert!(tracker.is_working("s1"));

        tracker.on_session_update(
            "s1",
            "tool_call_update",
            &json!({ "toolCallId": "t2", "status": "completed" }),
        );
        tracker.on_session_update(
            "s1",
            "agent_message_chunk",
            &json!({ "content": { "text": "All done" } }),
        );
        let Some(ProgressEffect::ScheduleSettle { generation: g2, .. }) =
            tracker.on_session_update("s1", "usage_update", &json!({ "used": 200 }))
        else {
            panic!("expected second settle schedule");
        };
        assert_ne!(g1, g2);

        let done = tracker.on_settle_timer("s1", g2);
        assert_eq!(
            done,
            Some(ProgressEffect::TurnCompleted {
                session_id: "s1".into(),
                stop_reason: "end_turn".into(),
                error: None,
            })
        );
        assert!(!tracker.is_working("s1"));
    }

    #[test]
    fn chained_background_tool_in_reactive_turn_stays_working() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1", 1);
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );
        tracker
            .on_prompt_replied("s1", 1, Ok(json!({ "stopReason": "end_turn" })))
            .unwrap();
        tracker.on_session_update(
            "s1",
            "tool_call_update",
            &json!({ "toolCallId": "bg1", "status": "completed" }),
        );
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg2", "status": "in_progress" }),
        );
        let effect = tracker.on_session_update("s1", "usage_update", &json!({ "used": 300 }));
        assert_eq!(
            effect,
            Some(ProgressEffect::TurnYielded {
                session_id: "s1".into()
            })
        );
        assert!(tracker.is_working("s1"));
    }

    #[test]
    fn cancel_during_background_wait_completes_session() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1", 1);
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );
        tracker
            .on_prompt_replied("s1", 1, Ok(json!({ "stopReason": "end_turn" })))
            .unwrap();

        assert_eq!(
            tracker.on_cancel("s1"),
            Some(ProgressEffect::TurnCompleted {
                session_id: "s1".into(),
                stop_reason: "cancelled".into(),
                error: None,
            })
        );
        assert!(!tracker.is_working("s1"));
    }
}
