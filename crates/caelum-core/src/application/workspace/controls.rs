use super::{Pending, Workspace};
use crate::application::{Command, ExportFormat, Reply, SessionKey};
use std::path::PathBuf;

#[derive(Default)]
pub(super) enum Status {
    #[default]
    Idle,
    Running(&'static str),
    Rewinding,
    Finished(String),
}

pub(super) enum Operation {
    Rename(String),
    Export(PathBuf),
}

impl super::Session {
    pub fn control_status(&self) -> Option<&str> {
        match &self.controls {
            Status::Idle => None,
            Status::Running(msg) => Some(msg),
            Status::Rewinding => Some("Rewinding conversation…"),
            Status::Finished(msg) => Some(msg),
        }
    }
    pub(super) fn rewinding(&self) -> bool {
        matches!(self.controls, Status::Rewinding)
    }
    pub(super) fn control_busy(&self) -> bool {
        matches!(self.controls, Status::Running(_) | Status::Rewinding)
    }
}

impl Workspace {
    pub fn rename_session(&mut self, index: usize, owner: &SessionKey, title: &str) -> bool {
        let title = title.trim();
        if !self.control_owner(index, owner) {
            return false;
        }
        if !super::titles::valid_title(title) {
            self.sessions[index].error =
                Some("Use a single-line title of 1–200 characters.".into());
            return false;
        }
        if self
            .pending
            .values()
            .any(|p| matches!(p, Pending::RegenerateTitle(_,key) if key == owner))
        {
            self.sessions[index].error = Some(
                "An automatic title update is finishing. Retry the rename in a moment.".into(),
            );
            return false;
        }
        self.sessions[index].controls = Status::Running("Saving session title…");
        self.request(
            Command::Rename(owner.clone(), title.into()),
            Pending::ConversationControl(index, owner.clone(), Operation::Rename(title.into())),
        );
        true
    }

    pub fn export_session(
        &mut self,
        index: usize,
        owner: &SessionKey,
        format: ExportFormat,
        path: PathBuf,
    ) -> bool {
        if !self.control_owner(index, owner) {
            return false;
        }
        let s = &mut self.sessions[index];
        if s.working() {
            s.error = Some("Wait for the current turn to finish before exporting.".into());
            return false;
        }
        s.controls = Status::Running("Exporting full provider history…");
        let command = Command::Export(owner.clone(), s.title.clone(), format, path.clone());
        self.request(
            command,
            Pending::ConversationControl(index, owner.clone(), Operation::Export(path)),
        );
        true
    }

    fn control_owner(&mut self, index: usize, owner: &SessionKey) -> bool {
        let Some(s) = self.sessions.get_mut(index) else {
            return false;
        };
        if s.key().as_ref() != Some(owner) || s.forgotten {
            return false;
        }
        if s.control_busy() || s.deleting || s.archiving || s.relocation.pending() || s.disconnected
        {
            s.error = Some("Wait for the session to finish its current operation and reconnect before retrying.".into());
            return false;
        }
        s.error = None;
        true
    }

    pub(super) fn finish_control(
        &mut self,
        index: usize,
        owner: SessionKey,
        op: Operation,
        result: Result<Reply, String>,
    ) {
        let s = &mut self.sessions[index];
        if s.key().as_ref() != Some(&owner) || s.forgotten {
            return;
        }
        s.controls = match result {
            Ok(Reply::Done) => Status::Finished(match op {
                Operation::Rename(title) => {
                    s.title = title;
                    s.title_state.manual = true;
                    s.title_state.turns = 0;
                    s.title_state.barrier = self.next_request;
                    "Session title saved.".into()
                }
                Operation::Export(path) => format!("Exported conversation to {}", path.display()),
            }),
            other => {
                s.error = Some(
                    other
                        .err()
                        .unwrap_or_else(|| "Unexpected provider response".into()),
                );
                Status::Idle
            }
        };
    }
}
