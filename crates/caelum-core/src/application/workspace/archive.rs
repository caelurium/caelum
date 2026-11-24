use super::{Pending, Session, Workspace};
use crate::application::{presentation::now, Command};

const NOTICE_DURATION_MS: u64 = 8_000;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Visibility {
    #[default]
    Active,
    Archived,
}

impl Visibility {
    fn from_archived(archived: bool) -> Self {
        if archived {
            Self::Archived
        } else {
            Self::Active
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::Active => Self::Archived,
            Self::Archived => Self::Active,
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Stable {
    visibility: Visibility,
    barrier: u64,
}

pub(super) enum Status {
    Stable(Stable),
    Changing {
        previous: Stable,
        request: u64,
        notice_revision: u64,
        selection: Option<(usize, u64)>,
    },
    Deleting {
        barrier: u64,
        request: u64,
    },
    Forgotten,
}

#[cfg(test)]
pub(in crate::application) enum ArchiveFixture {
    Stable(bool),
    Archiving,
    Deleting,
    Forgotten,
}

impl Default for Status {
    fn default() -> Self {
        Self::Stable(Stable::default())
    }
}

impl Session {
    pub fn archived(&self) -> bool {
        match &self.archive_status {
            Status::Stable(stable)
            | Status::Changing {
                previous: stable, ..
            } => stable.visibility == Visibility::Archived,
            Status::Deleting { .. } => true,
            Status::Forgotten => false,
        }
    }

    pub fn archiving(&self) -> bool {
        matches!(self.archive_status, Status::Changing { .. })
    }

    pub fn deleting(&self) -> bool {
        matches!(self.archive_status, Status::Deleting { .. })
    }

    pub fn forgotten(&self) -> bool {
        matches!(self.archive_status, Status::Forgotten)
    }

    pub(in crate::application::workspace) fn forget(&mut self) {
        self.archive_status = Status::Forgotten;
    }

    pub(super) fn restore_archive(&mut self, archived: bool, barrier: u64) {
        if !self.forgotten() {
            self.archive_status = Status::Stable(Stable {
                visibility: Visibility::from_archived(archived),
                barrier,
            });
        }
    }

    pub(super) fn observe_archive(&mut self, archived: bool, request: u64) {
        if let Status::Stable(stable) = &mut self.archive_status {
            if request > stable.barrier {
                stable.visibility = Visibility::from_archived(archived);
                stable.barrier = request;
            }
        }
    }

    #[cfg(test)]
    pub(in crate::application) fn archive_fixture(&mut self, phase: ArchiveFixture) {
        let stable = Stable::default();
        self.archive_status = match phase {
            ArchiveFixture::Stable(archived) => Status::Stable(Stable {
                visibility: Visibility::from_archived(archived),
                ..stable
            }),
            ArchiveFixture::Archiving => Status::Changing {
                previous: stable,
                request: 0,
                notice_revision: 0,
                selection: None,
            },
            ArchiveFixture::Deleting => Status::Deleting {
                request: 0,
                barrier: 0,
            },
            ArchiveFixture::Forgotten => Status::Forgotten,
        };
    }
}

pub(super) struct Notice {
    index: usize,
    replacement: Option<usize>,
    last_tick: u64,
    remaining: u64,
}

impl Workspace {
    pub fn archive(&mut self, index: usize, archived: bool) {
        self.set_archived(index, archived, true);
    }

    pub fn delete_archived_session(&mut self, index: usize) -> bool {
        let Some(session) = self.sessions.get(index) else {
            return false;
        };
        let Status::Stable(Stable {
            visibility: Visibility::Archived,
            barrier,
        }) = session.archive_status
        else {
            return false;
        };
        if session.working()
            || !session.queue.is_empty()
            || !session.inbox.items.is_empty()
            || self.approvals.iter().any(|prompt| {
                session.remote_id.is_some()
                    && session.remote_id.as_ref() == prompt.session_id.as_ref()
                    && session.scope.project == prompt.project_path
                    && session.scope.provider.as_str() == prompt.provider
            })
            || self.questions.iter().any(|prompt| {
                session.remote_id.as_deref() == Some(prompt.session_id.as_str())
                    && session.scope.project == prompt.project_path
                    && session.scope.provider.as_str() == prompt.provider
            })
        {
            return false;
        }
        let key = session.key();
        self.sessions[index].error = None;
        if let Some(key) = key {
            let request = self.request(Command::DeleteSession(key), Pending::DeleteSession(index));
            self.sessions[index].archive_status = Status::Deleting { barrier, request };
        } else {
            self.forget_deleted_session(index);
        }
        true
    }

    pub(super) fn forget_deleted_session(&mut self, index: usize) {
        if self.sessions[index].forgotten() {
            return;
        }
        let selected = self.selected == Some(index);
        if let Some(owner) = self.sessions[index].key() {
            self.revoke_approval_rules(&owner, None);
        }
        let session = &mut self.sessions[index];
        session.archive_status = Status::Forgotten;
        session.cancel_history_read();
        session.submission = Default::default();
        session.native_command = None;
        session.error = None;
        self.remembered.retain(|_, remembered| *remembered != index);
        self.remembered_checkouts
            .retain(|_, remembered| *remembered != index);
        self.dismiss_archive(index);
        if let Some(notice) = &mut self.archive_notice {
            if notice.replacement == Some(index) {
                notice.replacement = None;
            }
        }
        if selected {
            self.select_archive_replacement(index);
        }
    }

    pub fn undo_archive(&mut self) {
        if let Some(notice) = &self.archive_notice {
            let select = notice.replacement.is_some() && self.selected == notice.replacement;
            self.set_archived(notice.index, false, select);
        }
    }

    pub fn last_archived(&self) -> Option<usize> {
        self.archive_notice.as_ref().map(|notice| notice.index)
    }

    /// Presentation lifetime starts at successful archival, and pauses during interaction.
    pub fn tick_archive_notice(&mut self, now: u64, engaged: bool) -> bool {
        let Some(notice) = &mut self.archive_notice else {
            return false;
        };
        if !engaged && !self.sessions[notice.index].archiving() {
            notice.remaining = notice
                .remaining
                .saturating_sub(now.saturating_sub(notice.last_tick));
        }
        notice.last_tick = now;
        if notice.remaining == 0 {
            self.archive_notice = None;
            return true;
        }
        false
    }

    pub fn dismiss_archive(&mut self, index: usize) {
        if self.last_archived() == Some(index) {
            self.archive_notice = None;
        }
    }

    pub(super) fn reset_archive_notice(&mut self, index: usize) {
        if let Some(notice) = self
            .archive_notice
            .as_mut()
            .filter(|notice| notice.index == index)
        {
            notice.last_tick = now();
            notice.remaining = NOTICE_DURATION_MS;
        }
    }

    pub(super) fn set_notice_replacement(&mut self, index: usize, replacement: usize) {
        if let Some(notice) = self
            .archive_notice
            .as_mut()
            .filter(|notice| notice.index == index)
        {
            notice.replacement = Some(replacement);
        }
    }

    pub(super) fn archived_notice(&mut self, index: usize, revision: u64) -> bool {
        let project = &self.sessions[index].scope.project;
        if self.closed_projects.contains_key(self.group_path(project))
            && !self.project_is_open(project)
        {
            return false;
        }
        if revision < self.archive_notice_revision {
            return false;
        }
        self.archive_notice_revision = revision;
        self.archive_notice = Some(Notice {
            index,
            replacement: None,
            last_tick: now(),
            remaining: NOTICE_DURATION_MS,
        });
        true
    }

    pub(super) fn select_archive_replacement(&mut self, index: usize) -> usize {
        let scope = self.sessions[index].scope.clone();
        let group = self.group_path(&scope.project).to_owned();
        let replacement = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(candidate, session)| {
                *candidate != index
                    && !session.archived()
                    && !session.forgotten()
                    && !session.archiving()
                    && self.project_is_open(&session.scope.project)
                    && self.group_path(&session.scope.project) == group
            })
            .max_by_key(|(candidate, session)| {
                (
                    session.has_context(),
                    session.scope.project == scope.project,
                    *candidate,
                )
            })
            .map(|(candidate, _)| candidate);
        let replacement = replacement.unwrap_or_else(|| self.new_session(scope));
        self.select(replacement);
        replacement
    }

    fn set_archived(&mut self, index: usize, archived: bool, select: bool) {
        let Status::Stable(previous) = self.sessions[index].archive_status else {
            return;
        };
        if previous.visibility == Visibility::from_archived(archived) {
            return;
        }
        let needs_answer = self.approvals.iter().any(|p| {
            self.sessions[index].remote_id == p.session_id
                && self.sessions[index].scope.project == p.project_path
                && self.sessions[index].scope.provider.as_str() == p.provider
        }) || self.questions.iter().any(|p| {
            self.sessions[index].remote_id.as_ref() == Some(&p.session_id)
                && self.sessions[index].scope.project == p.project_path
                && self.sessions[index].scope.provider.as_str() == p.provider
        });
        let session = &mut self.sessions[index];
        if needs_answer
            || session.control_busy()
            || session.working()
            || !session.queue.is_empty()
            || !session.inbox.items.is_empty()
        {
            return;
        }
        session.error = None;
        self.archive_revision += 1;
        if let Some(key) = session.key() {
            let request = self.request(Command::Archive(key, archived), Pending::Archive(index));
            self.sessions[index].archive_status = Status::Changing {
                previous,
                request,
                notice_revision: self.archive_revision,
                selection: select.then(|| (self.selected.unwrap_or(index), self.navigation)),
            };
        } else {
            session.restore_archive(archived, self.next_request);
            if archived {
                let owns_notice = self.archived_notice(index, self.archive_revision);
                if self.selected == Some(index) {
                    let replacement = self.select_archive_replacement(index);
                    if owns_notice {
                        self.set_notice_replacement(index, replacement);
                    }
                }
            } else {
                self.dismiss_archive(index);
                if select {
                    self.select(index);
                }
            }
        }
    }

    pub(super) fn complete_archive(&mut self, index: usize, id: u64, result: Result<(), String>) {
        let session = &mut self.sessions[index];
        let Status::Changing {
            previous,
            request,
            notice_revision,
            selection,
        } = session.archive_status
        else {
            return;
        };
        if request != id {
            return;
        }
        let next = previous.visibility.opposite();
        if let Err(error) = result {
            session.archive_status = Status::Stable(previous);
            session.error = Some(error);
            if next == Visibility::Active {
                self.reset_archive_notice(index);
            }
            return;
        }
        session.archive_status = Status::Stable(Stable {
            visibility: next,
            barrier: self.next_request,
        });
        match next {
            Visibility::Archived => {
                let owns_notice = self.archived_notice(index, notice_revision);
                if self.selected == Some(index) {
                    let replacement = self.select_archive_replacement(index);
                    if owns_notice {
                        self.set_notice_replacement(index, replacement);
                    }
                }
            }
            Visibility::Active => {
                self.dismiss_archive(index);
                if !self.closed_since(&self.sessions[index].scope.project, request)
                    && selection.is_some_and(|(selected, navigation)| {
                        self.selected == Some(selected) && self.navigation == navigation
                    })
                {
                    self.select(index);
                }
            }
        }
    }

    pub(super) fn complete_delete(&mut self, index: usize, id: u64, result: Result<(), String>) {
        let session = &mut self.sessions[index];
        let Status::Deleting { barrier, request } = session.archive_status else {
            return;
        };
        if request != id {
            return;
        }
        match result {
            Ok(()) => self.forget_deleted_session(index),
            Err(error) => {
                session.restore_archive(true, barrier);
                session.error = Some(error);
            }
        }
    }
}
