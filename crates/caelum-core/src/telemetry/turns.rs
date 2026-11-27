use super::{Meter, Record};
use crate::{
    application::{Conversation, EntryKind, Provider, Scope},
    remote::{native, Speaker, Thread},
};
use caelum_protocol::StatsProvider;
use serde_json::Value;

#[derive(Default)]
pub(crate) struct Tracking(Option<Active>);

struct Active {
    id: String,
    started: u64,
    model: String,
    meter: Option<Meter>,
    last_total: Option<u64>,
}

impl Tracking {
    pub(crate) fn event(
        &mut self,
        scope: &Scope,
        session: &str,
        model: Option<&str>,
        method: &str,
        v: &Value,
        conversation: &Conversation,
    ) -> Option<Record> {
        match method {
            "turn/started" | "antigravity/turnStarted" | "session.execution.started" => {
                let at = at(v);
                let id = v["turn"]["id"]
                    .as_str()
                    .or(v["turnId"].as_str())
                    .or(v["executionID"].as_str())
                    .or(v["_event_id"].as_str())
                    .map(str::to_owned)
                    .unwrap_or_else(|| at.to_string());
                if self
                    .0
                    .as_ref()
                    .is_none_or(|a| a.started <= at && a.id != id)
                {
                    self.0 = Some(Active {
                        id,
                        started: at,
                        model: v["model"]
                            .as_str()
                            .or(model)
                            .unwrap_or("Provider default")
                            .into(),
                        meter: None,
                        last_total: None,
                    });
                }
                return None;
            }
            "session.step.started"
            | "session.step.ended"
            | "thread/tokenUsage/updated"
            | "antigravity/turnUsage"
            | "session.execution.succeeded"
            | "session.execution.failed"
            | "session.execution.interrupted"
            | "turn/completed"
            | "antigravity/turnCompleted" => {}
            _ => return None,
        }
        let active = self.0.as_mut()?;
        if v["turnId"]
            .as_str()
            .or(v["turn"]["id"].as_str())
            .or(v["executionID"].as_str())
            .is_some_and(|id| id != active.id)
            || v["_created"].as_u64().is_some_and(|at| at < active.started)
        {
            return None;
        }
        match method {
            "thread/tokenUsage/updated" => {
                let usage = &v["tokenUsage"];
                let total = usage["total"]["totalTokens"].as_u64();
                if total.is_none() || active.last_total.is_none_or(|old| total > Some(old)) {
                    active.add(Meter::inclusive(&usage["last"]));
                    active.last_total = total;
                }
            }
            "antigravity/turnUsage" => active.meter = Meter::inclusive(&v["usage"]),
            "session.step.started" | "session.step.ended" => {
                if let Some(id) = v["model"]["id"].as_str().or(v["modelID"].as_str()) {
                    active.model = id.into();
                }
                if method.ends_with("ended") {
                    active.add(Meter::step(v));
                }
            }
            _ => {
                let active = self.0.take()?;
                let timestamp = at(v);
                let estimated = active.meter.is_none();
                let meter = active.meter.unwrap_or_else(|| {
                    visible(conversation.entries.iter().rev().map(|e| {
                        (
                            e.kind() == EntryKind::User,
                            e.kind() == EntryKind::Assistant,
                            e.text.len(),
                        )
                    }))
                });
                let (provider, session) = if native::checkout_id(&scope.project).is_some() {
                    (
                        StatsProvider::Boat,
                        format!("native:{}:{session}", scope.provider.as_str()),
                    )
                } else {
                    (
                        match scope.provider {
                            Provider::Codex => StatsProvider::Codex,
                            Provider::OpenCode => StatsProvider::OpenCode,
                            Provider::Antigravity => StatsProvider::Antigravity,
                        },
                        session.into(),
                    )
                };
                return Some(Record {
                    project: scope.project.clone(),
                    provider,
                    session,
                    turn: active.id,
                    model: active.model,
                    timestamp,
                    duration: v["turn"]["durationMs"]
                        .as_u64()
                        .unwrap_or_else(|| timestamp.saturating_sub(active.started)),
                    meter,
                    estimated,
                });
            }
        }
        None
    }
}
impl Active {
    fn add(&mut self, m: Option<Meter>) {
        if let Some(m) = m {
            if let Some(old) = &mut self.meter {
                old.add(&m)
            } else {
                self.meter = Some(m)
            }
        }
    }
}

fn at(v: &Value) -> u64 {
    v["_created"]
        .as_u64()
        .unwrap_or_else(crate::application::presentation::now)
}

fn visible(lines: impl Iterator<Item = (bool, bool, usize)>) -> Meter {
    let mut output = 0usize;
    for (user, assistant, bytes) in lines {
        if user {
            return Meter::visible(bytes, output);
        }
        if assistant {
            output = output.saturating_add(bytes)
        }
    }
    Meter::visible(0, output)
}

pub(crate) fn boat(
    checkout: &str,
    t: &Thread,
    model: Option<String>,
    timestamp: u64,
) -> Option<Record> {
    let run = t.turn().run()?;
    Some(Record {
        project: native::project_key(checkout),
        provider: StatsProvider::Boat,
        session: format!("managed:{}:{}", run.provider.as_str(), run.conversation),
        turn: run.id.clone(),
        model: model
            .or_else(|| t.model.clone())
            .unwrap_or_else(|| format!("{} default", run.provider.name())),
        timestamp,
        duration: t
            .last_activity
            .map_or(0, |start| timestamp.saturating_sub(start)),
        meter: visible(t.lines.iter().rev().map(|l| {
            (
                l.speaker == Speaker::You,
                l.speaker.is_assistant(),
                l.text.len(),
            )
        })),
        estimated: true,
    })
}
