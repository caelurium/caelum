//! Provider-owned queued prompts. Identity survives admission retries and late snapshots.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxPrompt {
    pub id: String,
    pub text: String,
    phase: Phase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    Enqueuing(u64),
    Queued,
    EnqueueFailed(String),
    Removing(u64),
    RemoveFailed(String),
}

pub(super) enum Mutation {
    Enqueue,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedInboxPrompt {
    id: String,
    text: String,
    #[serde(default)]
    pending: bool,
    #[serde(default)]
    removing: bool,
    #[serde(default)]
    error: Option<String>,
}

impl InboxPrompt {
    pub fn pending(&self) -> bool {
        matches!(self.phase, Phase::Enqueuing(_) | Phase::Removing(_))
    }

    pub fn removing(&self) -> bool {
        matches!(self.phase, Phase::Removing(_) | Phase::RemoveFailed(_))
    }

    pub fn error(&self) -> Option<&str> {
        match &self.phase {
            Phase::EnqueueFailed(error) | Phase::RemoveFailed(error) => Some(error),
            _ => None,
        }
    }

    pub(super) fn saved(&self) -> SavedInboxPrompt {
        SavedInboxPrompt {
            id: self.id.clone(),
            text: self.text.clone(),
            pending: self.pending(),
            removing: self.removing(),
            error: self.error().map(str::to_owned),
        }
    }

    pub(super) fn matches_saved(&self, saved: &SavedInboxPrompt) -> bool {
        self.id == saved.id
            && self.text == saved.text
            && self.pending() == saved.pending
            && self.removing() == saved.removing
            && self.error() == saved.error.as_deref()
    }
}

impl SavedInboxPrompt {
    pub(super) fn restore(self) -> InboxPrompt {
        let error = if self.pending {
            Some("Delivery needs reconciliation. Refresh or retry.".into())
        } else {
            self.error
        };
        let phase = match (self.removing, error) {
            (true, Some(error)) => Phase::RemoveFailed(error),
            (false, Some(error)) => Phase::EnqueueFailed(error),
            (_, None) => Phase::Queued,
        };
        InboxPrompt {
            id: self.id,
            text: self.text,
            phase,
        }
    }
}

#[derive(Default)]
pub struct Inbox {
    pub items: Vec<InboxPrompt>,
    pub revision: u64,
    settled: std::collections::VecDeque<String>,
}

impl Inbox {
    pub(super) fn queued_text(&self) -> Option<&str> {
        self.items
            .iter()
            .find(|item| !matches!(item.phase, Phase::EnqueueFailed(_)))
            .map(|item| item.text.as_str())
    }

    pub(super) fn enqueue(&mut self, id: String, text: String, request: u64) {
        self.items.push(InboxPrompt {
            id,
            text,
            phase: Phase::Enqueuing(request),
        });
        self.revision += 1;
    }

    pub(super) fn start_mutation(&mut self, id: &str, request: u64, mutation: Mutation) {
        let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.id == id && !item.pending())
        else {
            return;
        };
        item.phase = match mutation {
            Mutation::Enqueue => Phase::Enqueuing(request),
            Mutation::Remove => Phase::Removing(request),
        };
        self.revision += 1;
    }

    pub(super) fn complete_mutation(&mut self, id: &str, request: u64, result: Result<(), String>) {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        let next = match (&item.phase, result) {
            (Phase::Enqueuing(owner), Ok(())) if *owner == request => Phase::Queued,
            (Phase::Enqueuing(owner), Err(error)) if *owner == request => {
                Phase::EnqueueFailed(error)
            }
            (Phase::Removing(owner), Err(error)) if *owner == request => Phase::RemoveFailed(error),
            (Phase::Removing(owner), Ok(())) if *owner == request => {
                self.settle(id);
                return;
            }
            _ => return,
        };
        item.phase = next;
        self.revision += 1;
    }

    pub(super) fn matches_saved(&self, saved: &[SavedInboxPrompt]) -> bool {
        self.items.len() == saved.len()
            && self
                .items
                .iter()
                .zip(saved)
                .all(|(item, saved)| item.matches_saved(saved))
    }

    pub fn settle(&mut self, id: &str) -> Option<InboxPrompt> {
        if !self.settled.iter().any(|v| v == id) {
            self.settled.push_back(id.into());
            if self.settled.len() > 256 {
                self.settled.pop_front();
            }
        }
        self.items.iter().position(|v| v.id == id).map(|i| {
            self.revision += 1;
            self.items.remove(i)
        })
    }

    pub fn snapshot(&mut self, revision: u64, values: &[Value]) {
        if revision != self.revision {
            return;
        }
        let mut items: Vec<_> = values
            .iter()
            .filter_map(parse)
            .filter(|v| !self.settled.contains(&v.id))
            .collect();
        for item in &self.items {
            if item.removing() {
                if let Some(remote) = items.iter_mut().find(|v| v.id == item.id) {
                    remote.phase.clone_from(&item.phase);
                }
            }
            if (item.pending() || item.error().is_some()) && !items.iter().any(|v| v.id == item.id)
            {
                items.push(item.clone());
            }
        }
        self.items = items;
    }

    pub fn event(&mut self, method: &str, data: &Value) -> Option<InboxPrompt> {
        let id = data["inboxID"].as_str()?;
        match method {
            "session.inbox.enqueued" => {
                self.revision += 1;
                if !self.settled.iter().any(|v| v == id) {
                    let mut item = data["item"].clone();
                    item["id"] = id.into();
                    if let Some(item) = parse(&item) {
                        if let Some(previous) = self.items.iter_mut().find(|v| v.id == id) {
                            if previous.removing() {
                                previous.text = item.text;
                            } else {
                                *previous = item;
                            }
                        } else {
                            self.items.push(item);
                        }
                    }
                }
            }
            "session.inbox.delivered" => return self.settle(id),
            "session.inbox.cancelled" => {
                self.settle(id);
            }
            _ => {}
        }
        None
    }
}

fn parse(value: &Value) -> Option<InboxPrompt> {
    if value["type"] != "user" {
        return None;
    }
    let text = value["payload"]["text"].as_str()?;
    let attachments = super::presentation::media(&value["payload"]["files"])
        .into_iter()
        .filter_map(|m| {
            if !m.source.starts_with("data:image/") {
                return None;
            }
            let mut a = super::Attachment::new(m.name, String::new(), None, false);
            a.content = m.source;
            a.kind = super::AttachmentKind::Image;
            Some(a)
        })
        .collect::<Vec<_>>();
    Some(InboxPrompt {
        id: value["id"].as_str()?.into(),
        text: super::composer::with_context(text, &attachments),
        phase: Phase::Queued,
    })
}
