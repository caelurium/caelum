use super::{Lane, Pending, Workspace};
use crate::{
    application::{Command, Reply, SessionKey},
    remote::{Chat, Connection, Continuation, Job, Machine, Picker, Size, Thread, Turn},
};

enum Origin {
    Home,
    Session {
        index: usize,
        key: Option<SessionKey>,
        revision: u64,
    },
}

/// Home may still have a restored session selected behind it. Its source
/// always comes from the user's choice on Home, never from that selection.
#[derive(Clone, Copy)]
pub enum HandoffSource {
    Composer(usize),
    HomeDraft,
    Recent(usize),
}

impl HandoffSource {
    pub fn session(self) -> Option<usize> {
        match self {
            Self::Composer(i) | Self::Recent(i) => Some(i),
            Self::HomeDraft => None,
        }
    }
}

pub enum HandoffPhase {
    Reading(u64),
    Ready(Chat),
    Failed(String),
}

pub struct VmHandoff {
    origin: Origin,
    navigation: u64,
    home_generation: Option<u64>,
    pub project: String,
    pub title: String,
    draft: String,
    pub phase: HandoffPhase,
}

pub enum VmDestination {
    Create {
        base: String,
        branch: String,
        size: Size,
        minutes: u32,
    },
    Existing(Machine),
}

impl VmHandoff {
    pub fn source(&self) -> HandoffSource {
        match self.origin {
            Origin::Home => HandoffSource::HomeDraft,
            Origin::Session { index, .. } if self.home_generation.is_some() => {
                HandoffSource::Recent(index)
            }
            Origin::Session { index, .. } => HandoffSource::Composer(index),
        }
    }
}

impl Workspace {
    pub fn handoff_blocker(&self, source: HandoffSource) -> Option<&'static str> {
        let Some(index) = source.session() else {
            return (self.home.opening() || !self.home_project_available())
                .then_some("Choose a project on Home to continue with its draft.");
        };
        let Some(s) = self.sessions.get(index) else {
            return Some("The session is no longer available.");
        };
        if (matches!(source, HandoffSource::Composer(_)) && self.selected != Some(index))
            || (matches!(source, HandoffSource::Recent(_))
                && self
                    .home
                    .draft
                    .project
                    .as_ref()
                    .is_some_and(|p| self.group_path(p) != self.group_path(&s.scope.project)))
            || !self.project_is_open(&s.scope.project)
            || s.archived
            || s.forgotten
            || s.archiving
            || s.deleting
            || s.relocation.pending()
        {
            Some("Choose an open local session.")
        } else if s.working()
            || s.stopping
            || s.commanding
            || !s.queue.is_empty()
            || !s.inbox.items.is_empty()
        {
            Some("Finish the current turn and queued prompts first.")
        } else if s.disconnected {
            Some("Reconnect the local session to read its chat.")
        } else {
            None
        }
    }

    pub fn begin_vm_handoff(&mut self, source: HandoffSource) -> bool {
        if self.handoff_blocker(source).is_some() {
            return false;
        }
        let (origin, project, title, draft, key) = if let Some(index) = source.session() {
            let s = &self.sessions[index];
            let key = s.key();
            (
                Origin::Session {
                    index,
                    key: key.clone(),
                    revision: s.revision,
                },
                s.scope.project.clone(),
                s.title.clone(),
                s.draft.clone(),
                key,
            )
        } else {
            let d = &self.home.draft;
            (
                Origin::Home,
                d.project.clone().unwrap(),
                "Home draft".into(),
                d.text.clone(),
                None,
            )
        };
        let phase = key.map_or_else(
            || HandoffPhase::Ready(Chat::default()),
            |key| {
                HandoffPhase::Reading(
                    self.request(Command::HandoffChat(key), Pending::Remote(Lane::Handoff)),
                )
            },
        );
        self.handoff = Some(VmHandoff {
            origin,
            navigation: self.navigation,
            home_generation: (!matches!(source, HandoffSource::Composer(_)))
                .then(|| self.home.generation()),
            project: project.clone(),
            title,
            draft,
            phase,
        });
        self.remote.picker = None;
        if self.remote.inventory.is_some() {
            self.remote_picker(project);
        } else if !matches!(self.remote.connection, Connection::Loading(_)) {
            self.list_machines();
        }
        true
    }

    pub fn vm_handoff(&self) -> Option<&VmHandoff> {
        self.handoff.as_ref().filter(|h| {
            h.navigation == self.navigation
                && h.home_generation
                    .is_none_or(|g| g == self.home.generation())
                && self.handoff_blocker(h.source()).is_none()
                && match &h.origin {
                    Origin::Home => self.home.draft.project.as_ref() == Some(&h.project),
                    Origin::Session {
                        index,
                        key,
                        revision,
                    } => {
                        self.sessions[*index].revision == *revision
                            && self.sessions[*index].scope.project == h.project
                            && self.sessions[*index].key() == *key
                    }
                }
        })
    }

    pub fn dismiss_vm_handoff(&mut self) {
        if self.handoff.take().is_some() {
            self.remote.picker = None;
        }
    }

    pub(super) fn complete_handoff_chat(&mut self, request: u64, result: Result<Reply, String>) {
        if self
            .vm_handoff()
            .is_some_and(|h| matches!(h.phase, HandoffPhase::Reading(r) if r == request))
        {
            self.handoff.as_mut().unwrap().phase = match result {
                Ok(Reply::HandoffChat(chat)) => HandoffPhase::Ready(chat),
                other => HandoffPhase::Failed(
                    other
                        .err()
                        .unwrap_or_else(|| "Unexpected session history response".into()),
                ),
            };
        }
    }

    pub fn handoff_machine_blocker(&self, machine: &Machine) -> Option<&'static str> {
        let Some(Picker::Ready(source)) = &self.remote.picker else {
            return Some("Choose a project first.");
        };
        if machine.key.account != source.account {
            return Some("This VM belongs to another Boat account.");
        }
        let (id, c) = self
            .remote
            .saved
            .checkouts
            .iter()
            .find(|(_, c)| c.machine().is_some_and(|m| m.key == machine.key))?;
        if !c.source.repository.eq_ignore_ascii_case(&source.repository) {
            Some("This VM has a different repository.")
        } else {
            (!self.can_switch_vm_thread(id)).then_some("Finish the current VM operation first.")
        }
    }

    /// Commit the captured source to exactly one checkout before dispatching
    /// any machine work. Provisioning retries reuse that checkout and thread.
    pub fn finish_vm_handoff(&mut self, destination: VmDestination) -> Option<String> {
        let h = self.vm_handoff()?;
        let Some(Picker::Ready(source)) = &self.remote.picker else {
            return None;
        };
        let inventory = self.remote.inventory.as_ref()?;
        if !matches!(h.phase, HandoffPhase::Ready(_))
            || source.project != self.group_path(&h.project)
            || source.account != inventory.account
        {
            return None;
        }
        let id = match destination {
            VmDestination::Create {
                base,
                branch,
                size,
                minutes,
            } => self.create_remote(&base, &branch, size, minutes)?,
            VmDestination::Existing(m) => {
                if self.handoff_machine_blocker(&m).is_some()
                    || !inventory.machines.iter().any(|v| v.key == m.key)
                {
                    return None;
                }
                self.attach_remote(m)?
            }
        };
        let h = self.handoff.take().unwrap();
        let HandoffPhase::Ready(chat) = h.phase else {
            unreachable!()
        };
        let c = self.remote.saved.checkouts.get_mut(&id).unwrap();
        let prior = std::mem::replace(
            &mut c.thread,
            Thread {
                continued: Some(Continuation {
                    id: uuid::Uuid::new_v4().to_string(),
                    title: h.title,
                    chat,
                }),
                draft: h.draft,
                ..Default::default()
            },
        );
        if prior != Thread::default() {
            c.other_threads.push(prior);
        }
        self.remote.feeds.remove(&id);
        Some(id)
    }

    pub fn can_switch_vm_thread(&self, id: &str) -> bool {
        self.remote
            .saved
            .checkouts
            .get(id)
            .is_some_and(|c| c.thread.turn == Turn::Idle)
            && !matches!(self.remote.jobs.get(id), Some(Job::Busy(_) | Job::Wait(_)))
            && !matches!(self.remote.feeds.get(id), Some(Job::Busy(_) | Job::Wait(_)))
            && !self.remote.interrupts.contains_key(id)
            && !self.publishing(id)
    }

    pub fn switch_vm_thread(&mut self, id: &str, thread: &str) -> bool {
        if self.remote.saved.selected.as_deref() != Some(id) || !self.can_switch_vm_thread(id) {
            return false;
        }
        let c = self.remote.saved.checkouts.get_mut(id).unwrap();
        let Some(other) = c
            .other_threads
            .iter_mut()
            .find(|t| t.editor_key(id) == thread)
        else {
            return false;
        };
        std::mem::swap(&mut c.thread, other);
        self.remote.feeds.remove(id);
        true
    }
}
