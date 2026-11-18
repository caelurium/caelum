//! Provider-backed conversation forks. Picker pages are read independently of the parent's transcript.
use super::{update_summary, Pending, Session, Workspace};
use crate::application::{presentation, split_context, Command, Provider, Reply, SessionKey};
use caelum_protocol::SessionHistoryPage;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForkBoundary {
    CodexTurn(String),
    OpenCodeBefore(String),
    OpenCodeTip(String),
}

#[derive(Debug)]
pub struct ForkPoint {
    pub label: String,
    pub boundary: ForkBoundary,
    pub newer_turns: usize,
}

enum Phase {
    Ready,
    Reading(u64),
    Creating(u64),
}

pub struct ForkPicker {
    index: usize,
    source: SessionKey,
    navigation: u64,
    phase: Phase,
    pub points: Vec<ForkPoint>,
    seen_turns: usize,
    pub error: Option<String>,
    cursor: Option<String>,
    // OpenCode pages are newest first. Carry the following user prompt across a page break.
    newer_prompt: Option<String>,
    tip: Option<String>,
}

impl ForkPicker {
    pub fn loading(&self) -> bool {
        matches!(self.phase, Phase::Reading(_))
    }
    pub fn creating(&self) -> bool {
        matches!(self.phase, Phase::Creating(_))
    }
    pub fn has_older(&self) -> bool {
        self.cursor.is_some()
    }

    fn replace_page(&mut self, page: SessionHistoryPage) {
        if page.next_cursor.is_some() && page.next_cursor == self.cursor {
            self.cursor = None;
            self.error =
                Some("The provider returned the same history page. Reopen /fork to reload.".into());
            return;
        }
        self.cursor = page.next_cursor;
        self.points.clear();
        for record in page.data.iter().rev() {
            let Some(id) = record["id"].as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            let (boundary, prompt) = match self.source.scope.provider {
                Provider::Codex => {
                    let status = record["status"]
                        .as_str()
                        .or(record["status"]["type"].as_str());
                    if !matches!(status, Some("completed" | "failed" | "interrupted")) {
                        continue;
                    }
                    let Some(user) = record["items"]
                        .as_array()
                        .and_then(|items| items.iter().find(|i| i["type"] == "userMessage"))
                    else {
                        continue;
                    };
                    let text = user["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|item| item["text"].as_str())
                        .collect::<Vec<_>>()
                        .join(" ");
                    (ForkBoundary::CodexTurn(id.into()), text)
                }
                Provider::OpenCode => {
                    let tip = self.tip.get_or_insert_with(|| id.to_owned());
                    if record["type"] != "user" {
                        continue;
                    }
                    let boundary = self.newer_prompt.replace(id.into()).map_or_else(
                        || ForkBoundary::OpenCodeTip(tip.clone()),
                        ForkBoundary::OpenCodeBefore,
                    );
                    (
                        boundary,
                        record["text"].as_str().unwrap_or_default().to_owned(),
                    )
                }
            };
            let shown = presentation::visible_text(split_context(&prompt).0);
            let label = shown
                .lines()
                .map(str::trim)
                .find(|s| !s.is_empty())
                .unwrap_or("Attached context")
                .chars()
                .take(180)
                .collect();
            self.points.push(ForkPoint {
                label,
                boundary,
                newer_turns: self.seen_turns,
            });
            self.seen_turns += 1;
        }
    }
}

pub(super) enum Request {
    Page,
    Create { index: usize, source: SessionKey },
}

impl Workspace {
    pub fn fork_picker(&self) -> Option<&ForkPicker> {
        self.fork.as_ref().filter(|p| {
            self.selected == Some(p.index)
                && self.navigation == p.navigation
                && self.sessions[p.index].key().as_ref() == Some(&p.source)
        })
    }

    pub fn dismiss_fork(&mut self) {
        self.fork = None;
    }

    fn fork_source(&self, index: usize) -> Result<SessionKey, &'static str> {
        let s = &self.sessions[index];
        let key = s
            .key()
            .ok_or("Send a message before forking this conversation.")?;
        if self.selected != Some(index)
            || s.archived
            || s.forgotten
            || s.deleting
            || s.archiving
            || s.relocation.pending()
            || !self.project_is_open(&s.scope.project)
        {
            return Err("Open an active conversation to fork it.");
        }
        if s.disconnected {
            return Err("Reconnect the session before forking.");
        }
        if self
            .pending
            .values()
            .any(|p| matches!(p, Pending::Fork(Request::Create { index: i, .. }) if *i == index))
        {
            return Err("A fork is already in progress.");
        }
        if s.working()
            || s.stopping
            || s.commanding
            || !s.queue.is_empty()
            || !s.inbox.items.is_empty()
            || self.approvals.iter().any(|p| {
                p.provider == key.scope.provider.as_str()
                    && p.project_path == key.scope.project
                    && p.session_id.as_ref().is_none_or(|id| id == &key.id)
            })
            || self.questions.iter().any(|p| {
                p.provider == key.scope.provider.as_str()
                    && p.project_path == key.scope.project
                    && p.session_id == key.id
            })
        {
            return Err("Finish the current turn and resolve queued prompts or input requests before forking.");
        }
        Ok(key)
    }

    pub fn open_fork(&mut self, index: usize) -> bool {
        let source = match self.fork_source(index) {
            Ok(key) => key,
            Err(error) => {
                self.sessions[index].error = Some(error.into());
                return false;
            }
        };
        let request = self.request(
            Command::History(source.clone(), None),
            Pending::Fork(Request::Page),
        );
        self.sessions[index].error = None;
        self.fork = Some(ForkPicker {
            index,
            source,
            navigation: self.navigation,
            phase: Phase::Reading(request),
            points: vec![],
            seen_turns: 0,
            error: None,
            cursor: None,
            newer_prompt: None,
            tip: None,
        });
        true
    }

    pub fn older_fork_points(&mut self) {
        let Some(picker) = self
            .fork_picker()
            .filter(|p| matches!(p.phase, Phase::Ready))
        else {
            return;
        };
        let Some(cursor) = picker.cursor.clone() else {
            return;
        };
        let command = Command::History(picker.source.clone(), Some(cursor));
        let id = self.request(command, Pending::Fork(Request::Page));
        let picker = self.fork.as_mut().unwrap();
        picker.phase = Phase::Reading(id);
        picker.error = None;
    }

    pub fn fork_at(&mut self, boundary: &ForkBoundary) {
        let Some(picker) = self.fork_picker().filter(|p| {
            matches!(p.phase, Phase::Ready)
                && p.points.iter().any(|point| &point.boundary == boundary)
        }) else {
            return;
        };
        let index = picker.index;
        let source = match self.fork_source(index) {
            Ok(key) => key,
            Err(error) => {
                self.fork.as_mut().unwrap().error = Some(error.into());
                return;
            }
        };
        let request = self.request(
            Command::Fork(source.clone(), boundary.clone()),
            Pending::Fork(Request::Create { index, source }),
        );
        let picker = self.fork.as_mut().unwrap();
        picker.error = None;
        picker.phase = Phase::Creating(request);
    }

    pub(super) fn complete_fork(
        &mut self,
        request: u64,
        pending: Request,
        result: Result<Reply, String>,
    ) {
        match pending {
            Request::Page => {
                if !self
                    .fork_picker()
                    .is_some_and(|p| matches!(p.phase, Phase::Reading(id) if id == request))
                {
                    return;
                }
                let picker = self.fork.as_mut().unwrap();
                picker.phase = Phase::Ready;
                match result {
                    Ok(Reply::History(page)) => picker.replace_page(page),
                    other => {
                        picker.error = Some(
                            other
                                .err()
                                .unwrap_or_else(|| "Unexpected history response".into()),
                        )
                    }
                }
            }
            Request::Create { index, source } => {
                let foreground = self
                    .fork_picker()
                    .is_some_and(|p| matches!(p.phase, Phase::Creating(id) if id == request));
                let child = match result {
                    Ok(Reply::Forked {
                        source: owner,
                        child,
                    }) if owner == source
                        && child.key.scope == source.scope
                        && !child.key.id.is_empty()
                        && child.key.id != source.id =>
                    {
                        child
                    }
                    other => {
                        let error = other
                            .err()
                            .unwrap_or_else(|| "Provider returned a different fork".into());
                        if foreground {
                            let picker = self.fork.as_mut().unwrap();
                            picker.phase = Phase::Ready;
                            picker.error = Some(error);
                        } else if self.sessions[index].key().as_ref() == Some(&source)
                            && !self.sessions[index].forgotten
                        {
                            self.sessions[index].error = Some(error);
                        }
                        return;
                    }
                };
                // Discovery may have already created an editor for the child. Never replace it.
                let child_index = match self
                    .sessions
                    .iter()
                    .position(|s| s.key().as_ref() == Some(&child.key))
                {
                    Some(i) => i,
                    None => {
                        let mut s = Session::new(source.scope.clone());
                        s.remote_id = Some(child.key.id.clone());
                        s.loaded = false;
                        update_summary(&mut s, child, request);
                        self.sessions.push(s);
                        let i = self.sessions.len() - 1;
                        if self.project_is_open(&source.scope.project) {
                            self.detail_queue.push(i);
                        }
                        i
                    }
                };
                if foreground {
                    self.dismiss_fork();
                    if !self.closed_since(&source.scope.project, request)
                        && !self.sessions[child_index].archived
                        && !self.sessions[child_index].forgotten
                    {
                        self.select(child_index);
                    }
                }
            }
        }
    }
}
