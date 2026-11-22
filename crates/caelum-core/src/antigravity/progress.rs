use std::{
    collections::{HashMap, HashSet},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Prompting,
    WaitingBackground,
    ReactiveTurn { settle_generation: Option<u64> },
}

#[derive(Debug)]
struct SessionProgress {
    phase: Phase,
    active_tools: HashSet<String>,
}

impl Default for SessionProgress {
    fn default() -> Self {
        Self {
            phase: Phase::Idle,
            active_tools: HashSet::new(),
        }
    }
}

/// Tracks per-session Antigravity turn lifecycle across foreground prompts,
/// background tools that outlive `session/prompt`, and reactive follow-up turns.
#[derive(Debug, Default)]
pub(super) struct SessionProgressTracker {
    sessions: HashMap<String, SessionProgress>,
    next_generation: u64,
}

impl SessionProgressTracker {
    pub(super) fn is_working(&self, session_id: &str) -> bool {
        self.sessions
            .get(session_id)
            .is_some_and(|session| session.phase != Phase::Idle)
    }

    pub(super) fn working_sessions(&self) -> HashSet<String> {
        self.sessions
            .iter()
            .filter(|(_, session)| session.phase != Phase::Idle)
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub(super) fn on_prompt_started(&mut self, session_id: &str) {
        let session = self.sessions.entry(session_id.to_owned()).or_default();
        session.phase = Phase::Prompting;
    }

    pub(super) fn on_prompt_send_failed(&mut self, session_id: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.phase = if session.active_tools.is_empty() {
                Phase::Idle
            } else {
                Phase::WaitingBackground
            };
        }
    }

    pub(super) fn on_prompt_replied(
        &mut self,
        session_id: &str,
        result: Result<Value, String>,
    ) -> ProgressEffect {
        let session = self.sessions.entry(session_id.to_owned()).or_default();
        match result {
            Err(error) => {
                session.active_tools.clear();
                session.phase = Phase::Idle;
                ProgressEffect::TurnCompleted {
                    session_id: session_id.to_owned(),
                    stop_reason: "error".into(),
                    error: Some(error),
                }
            }
            Ok(res) => {
                let stop_reason = res["stopReason"].as_str().unwrap_or("end_turn").to_owned();
                if stop_reason == "end_turn" && !session.active_tools.is_empty() {
                    session.phase = Phase::WaitingBackground;
                    ProgressEffect::TurnYielded {
                        session_id: session_id.to_owned(),
                    }
                } else {
                    session.active_tools.clear();
                    session.phase = Phase::Idle;
                    ProgressEffect::TurnCompleted {
                        session_id: session_id.to_owned(),
                        stop_reason,
                        error: None,
                    }
                }
            }
        }
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
                let tool_id = update["toolCallId"].as_str().filter(|s| !s.is_empty())?;
                let active = is_tool_update_active(kind, update["status"].as_str());
                let session = self.sessions.entry(session_id.to_owned()).or_default();
                if active {
                    session.active_tools.insert(tool_id.to_owned());
                    match session.phase {
                        Phase::Idle => session.phase = Phase::WaitingBackground,
                        Phase::ReactiveTurn { .. } => {
                            session.phase = Phase::ReactiveTurn {
                                settle_generation: None,
                            };
                        }
                        Phase::Prompting | Phase::WaitingBackground => {}
                    }
                } else {
                    let was_active = session.active_tools.remove(tool_id);
                    if session.phase == Phase::WaitingBackground
                        && session.active_tools.is_empty()
                        && was_active
                    {
                        session.phase = Phase::ReactiveTurn {
                            settle_generation: None,
                        };
                    }
                }
                None
            }
            "agent_thought_chunk" | "agent_message_chunk" | "plan" => {
                let session = self.sessions.entry(session_id.to_owned()).or_default();
                match session.phase {
                    Phase::Idle => {
                        session.phase = Phase::ReactiveTurn {
                            settle_generation: None,
                        };
                    }
                    Phase::ReactiveTurn {
                        settle_generation: Some(_),
                    } => {
                        session.phase = Phase::ReactiveTurn {
                            settle_generation: None,
                        };
                    }
                    Phase::Prompting
                    | Phase::WaitingBackground
                    | Phase::ReactiveTurn {
                        settle_generation: None,
                    } => {}
                }
                None
            }
            "usage_update" => {
                let phase = self.sessions.get(session_id)?.phase;
                let has_active_tools = !self.sessions.get(session_id)?.active_tools.is_empty();
                match phase {
                    Phase::WaitingBackground if has_active_tools => {
                        Some(ProgressEffect::TurnYielded {
                            session_id: session_id.to_owned(),
                        })
                    }
                    Phase::ReactiveTurn { .. } if has_active_tools => {
                        if let Some(session) = self.sessions.get_mut(session_id) {
                            session.phase = Phase::WaitingBackground;
                        }
                        Some(ProgressEffect::TurnYielded {
                            session_id: session_id.to_owned(),
                        })
                    }
                    Phase::ReactiveTurn { .. } => {
                        self.next_generation = self.next_generation.wrapping_add(1).max(1);
                        let generation = self.next_generation;
                        if let Some(session) = self.sessions.get_mut(session_id) {
                            session.phase = Phase::ReactiveTurn {
                                settle_generation: Some(generation),
                            };
                        }
                        Some(ProgressEffect::ScheduleSettle {
                            session_id: session_id.to_owned(),
                            generation,
                        })
                    }
                    Phase::Idle | Phase::Prompting | Phase::WaitingBackground => None,
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
        let session = self.sessions.get_mut(session_id)?;
        if session.phase
            == (Phase::ReactiveTurn {
                settle_generation: Some(generation),
            })
            && session.active_tools.is_empty()
        {
            session.phase = Phase::Idle;
            Some(ProgressEffect::TurnCompleted {
                session_id: session_id.to_owned(),
                stop_reason: "end_turn".into(),
                error: None,
            })
        } else {
            None
        }
    }

    pub(super) fn on_cancel(&mut self, session_id: &str) -> Option<ProgressEffect> {
        let session = self.sessions.get_mut(session_id)?;
        match session.phase {
            Phase::WaitingBackground | Phase::ReactiveTurn { .. } => {
                session.active_tools.clear();
                session.phase = Phase::Idle;
                Some(ProgressEffect::TurnCompleted {
                    session_id: session_id.to_owned(),
                    stop_reason: "cancelled".into(),
                    error: None,
                })
            }
            Phase::Idle | Phase::Prompting => None,
        }
    }

    pub(super) fn on_replayed_history(&mut self, session_id: &str, active_tools: Vec<String>) {
        if active_tools.is_empty() {
            return;
        }
        let session = self.sessions.entry(session_id.to_owned()).or_default();
        if session.phase == Phase::Idle {
            session.active_tools.extend(active_tools);
            session.phase = Phase::WaitingBackground;
        }
    }

    pub(super) fn on_disconnected(&mut self) -> Vec<ProgressEffect> {
        let mut effects = Vec::new();
        for (session_id, session) in self.sessions.drain() {
            if matches!(
                session.phase,
                Phase::WaitingBackground | Phase::ReactiveTurn { .. }
            ) {
                effects.push(ProgressEffect::TurnCompleted {
                    session_id,
                    stop_reason: "disconnected".into(),
                    error: Some("Antigravity ACP connection closed".into()),
                });
            }
        }
        effects
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
    fn synchronous_turn_completes_immediately_on_prompt_reply() {
        let mut tracker = SessionProgressTracker::default();
        tracker.on_prompt_started("s1");
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
        let effect = tracker.on_prompt_replied("s1", Ok(json!({ "stopReason": "end_turn" })));
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
        tracker.on_prompt_started("s1");
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );

        let effect = tracker.on_prompt_replied("s1", Ok(json!({ "stopReason": "end_turn" })));
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
        tracker.on_prompt_started("s1");
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );
        tracker.on_prompt_replied("s1", Ok(json!({ "stopReason": "end_turn" })));
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
        tracker.on_prompt_started("s1");
        tracker.on_session_update(
            "s1",
            "tool_call",
            &json!({ "toolCallId": "bg1", "status": "in_progress" }),
        );
        tracker.on_prompt_replied("s1", Ok(json!({ "stopReason": "end_turn" })));

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
