use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use super::AntigravityError;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(from = "SavedRecord", into = "SavedRecord")]
struct SessionRecord {
    visibility: Visibility,
    title: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Visibility {
    #[default]
    Open,
    Archived,
    Deleted,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SavedRecord {
    archived: bool,
    deleted: bool,
    title: Option<String>,
}

impl From<SavedRecord> for SessionRecord {
    fn from(saved: SavedRecord) -> Self {
        let visibility = match (saved.deleted, saved.archived) {
            (true, _) => Visibility::Deleted,
            (false, true) => Visibility::Archived,
            (false, false) => Visibility::Open,
        };
        Self {
            visibility,
            title: saved.title,
        }
    }
}

impl From<SessionRecord> for SavedRecord {
    fn from(record: SessionRecord) -> Self {
        Self {
            archived: record.visibility == Visibility::Archived,
            deleted: record.visibility == Visibility::Deleted,
            title: record.title,
        }
    }
}

/// Persists archive, delete, and custom title metadata for Antigravity ACP sessions,
/// since `agy acp` does not expose archive/delete/rename RPC methods.
#[derive(Default)]
pub(crate) struct SessionMetadataStore {
    path: Option<PathBuf>,
    projects: BTreeMap<String, BTreeMap<String, SessionRecord>>,
}

impl SessionMetadataStore {
    pub(crate) fn open(&mut self, path: PathBuf) -> Result<(), String> {
        match fs::read(&path) {
            Ok(bytes) => {
                let loaded: BTreeMap<String, BTreeMap<String, SessionRecord>> =
                    serde_json::from_slice(&bytes).map_err(|e| {
                        format!(
                            "Cannot read Antigravity session metadata '{}': {e}",
                            path.display()
                        )
                    })?;
                for (project, sessions) in loaded {
                    let target = self.projects.entry(project).or_default();
                    for (id, record) in sessions {
                        target.entry(id).or_insert(record);
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.to_string()),
        }
        self.path = Some(path);
        Ok(())
    }

    pub(crate) fn is_archived(&self, project: &Path, session_id: &str) -> bool {
        self.record(project, session_id)
            .is_some_and(|r| r.visibility == Visibility::Archived)
    }

    pub(crate) fn is_deleted(&self, project: &Path, session_id: &str) -> bool {
        self.record(project, session_id)
            .is_some_and(|r| r.visibility == Visibility::Deleted)
    }

    pub(crate) fn title(&self, project: &Path, session_id: &str) -> Option<String> {
        self.record(project, session_id)
            .and_then(|r| r.title.clone())
            .filter(|t| !t.trim().is_empty())
    }

    pub(crate) fn set_archived(
        &mut self,
        project: &Path,
        session_id: &str,
        archived: bool,
    ) -> Result<(), AntigravityError> {
        let record = self.entry_mut(project, session_id);
        if record.visibility == Visibility::Deleted {
            return Err(AntigravityError::Protocol(
                "Antigravity session has been deleted".into(),
            ));
        }
        record.visibility = if archived {
            Visibility::Archived
        } else {
            Visibility::Open
        };
        self.save()
    }

    pub(crate) fn mark_deleted(
        &mut self,
        project: &Path,
        session_id: &str,
    ) -> Result<(), AntigravityError> {
        let record = self.entry_mut(project, session_id);
        record.visibility = Visibility::Deleted;
        self.save()
    }

    pub(crate) fn set_title(
        &mut self,
        project: &Path,
        session_id: &str,
        title: &str,
    ) -> Result<(), AntigravityError> {
        let trimmed = title.trim();
        let record = self.entry_mut(project, session_id);
        record.title = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        };
        self.save()
    }

    fn record(&self, project: &Path, session_id: &str) -> Option<&SessionRecord> {
        let key = project.to_string_lossy();
        self.projects.get(key.as_ref())?.get(session_id)
    }

    fn entry_mut(&mut self, project: &Path, session_id: &str) -> &mut SessionRecord {
        self.projects
            .entry(project.to_string_lossy().into_owned())
            .or_default()
            .entry(session_id.to_owned())
            .or_default()
    }

    fn save(&self) -> Result<(), AntigravityError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let payload = serde_json::to_vec_pretty(&self.projects)
            .map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        let mut file = options
            .open(&temporary)
            .map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        file.write_all(&payload)
            .map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        file.sync_all()
            .map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        fs::rename(&temporary, path).map_err(|e| AntigravityError::Protocol(e.to_string()))?;
        Ok(())
    }
}
