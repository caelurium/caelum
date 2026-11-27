use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use caelum_protocol::{PersistedProject, ProjectInfo, WorkspaceSnapshot};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use thiserror::Error;

use crate::blocking_actor::BlockingActor;

mod migrations;
use migrations::migrate;
#[cfg(test)]
use migrations::CURRENT_SCHEMA_VERSION;
const PROVIDERS: [&str; 2] = ["codex", "opencode"];
const BRANCH_PROVIDERS: [&str; 3] = ["codex", "opencode", "antigravity"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionBranchHistory {
    pub branch: Option<String>,
    pub branches: Vec<String>,
}

/// Thread-safe access to Caelum's app-owned workspace metadata database.
/// A dedicated worker owns the SQLite connection. Calls wait for completion.
/// Provider conversation history is deliberately never stored here.
#[derive(Clone)]
pub struct WorkspaceDatabase {
    connection: BlockingActor<Connection>,
}

impl WorkspaceDatabase {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| DatabaseError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut connection)?;

        Ok(Self {
            connection: BlockingActor::spawn("caelum-workspace-db", connection),
        })
    }

    pub fn restore(&self) -> Result<WorkspaceSnapshot, DatabaseError> {
        self.call(move |connection| {
            let mut statement = connection.prepare(
                "SELECT path, name, provider, root_path, selected_worktree_path
                 FROM projects
                 WHERE is_open = 1
                 ORDER BY open_order ASC, last_opened_at DESC, path ASC",
            )?;
            let rows = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?;

            let mut projects = Vec::new();
            for row in rows {
                let (path, name, provider, root_path, selected_worktree_path) = row?;
                validate_provider(&provider)?;
                let mut provider_statement = connection.prepare(
                    "SELECT provider, selected_session_id
                     FROM project_provider_state
                     WHERE project_path = ?1",
                )?;
                let provider_rows = provider_statement.query_map([&path], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?;

                let (mut codex_session_id, mut opencode_session_id) = (None, None);
                for provider_row in provider_rows {
                    let (provider, session_id) = provider_row?;
                    match provider.as_str() {
                        "codex" => codex_session_id = session_id,
                        "opencode" => opencode_session_id = session_id,
                        _ => return Err(DatabaseError::UnsupportedProvider(provider)),
                    }
                }

                let mut session_statement = connection.prepare(
                    "SELECT provider, session_id
                     FROM sessions
                     WHERE project_path = ?1
                     ORDER BY COALESCE(last_opened_at, 0) DESC, first_seen_at DESC, session_id ASC",
                )?;
                let session_rows = session_statement.query_map([&path], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                let (mut codex_session_ids, mut opencode_session_ids) = (Vec::new(), Vec::new());
                for session_row in session_rows {
                    let (provider, session_id) = session_row?;
                    match provider.as_str() {
                        "codex" => codex_session_ids.push(session_id),
                        "opencode" => opencode_session_ids.push(session_id),
                        _ => return Err(DatabaseError::UnsupportedProvider(provider)),
                    }
                }

                projects.push(PersistedProject {
                    project: ProjectInfo { name, path },
                    root_path,
                    selected_worktree_path,
                    provider,
                    codex_session_id,
                    opencode_session_id,
                    codex_session_ids,
                    opencode_session_ids,
                });
            }

            let active_project_path = connection
                .query_row(
                    "SELECT value FROM app_metadata WHERE key = 'active_project_path'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .filter(|path| projects.iter().any(|project| project.project.path == *path));

            Ok(WorkspaceSnapshot {
                projects,
                active_project_path,
            })
        })
    }

    /// Record a project as open and move it to the front of the restore order.
    pub fn remember_project(&self, project: &ProjectInfo) -> Result<(), DatabaseError> {
        let project = project.clone();
        self.call(move |connection| {
            let project = &project;
            let transaction = connection.transaction()?;
            remember_project_in_transaction(&transaction, project)?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Keep provider metadata keyed by the checkout directory while grouping it
    /// under its original project. The caller validates Git membership first.
    pub fn remember_worktree(
        &self,
        root: &ProjectInfo,
        checkout: &ProjectInfo,
    ) -> Result<(), DatabaseError> {
        let root = root.clone();
        let checkout = checkout.clone();
        self.call(move |connection| {
            let root = &root;
            let checkout = &checkout;
            let transaction = connection.transaction()?;
            remember_project_in_transaction(&transaction, root)?;
            if root.path != checkout.path {
                remember_project_in_transaction(&transaction, checkout)?;
                transaction.execute(
                    "UPDATE projects SET root_path = ?2 WHERE path = ?1",
                    params![checkout.path, root.path],
                )?;
                transaction.execute(
                    "UPDATE projects SET selected_worktree_path = ?1 WHERE path = ?2
                     AND EXISTS(SELECT 1 FROM app_metadata WHERE key = 'active_project_path' AND value = ?1)",
                    params![checkout.path, root.path],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn remove_worktree(&self, checkout: &str) -> Result<(), DatabaseError> {
        let checkout = checkout.to_owned();
        self.call(move |connection| {
            let checkout = &checkout;
            let transaction = connection.transaction()?;
            let directories = {
                let mut statement = transaction.prepare("SELECT path, root_path FROM projects")?;
                let rows = statement.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for (path, root) in directories
                .into_iter()
                .filter(|(path, _)| Path::new(path).starts_with(checkout))
            {
                transaction.execute(
                    "UPDATE projects SET is_open = 0, root_path = NULL WHERE path = ?1",
                    [&path],
                )?;
                transaction.execute("UPDATE projects SET selected_worktree_path = NULL WHERE selected_worktree_path = ?1", [&path])?;
                if let Some(root) = root {
                    transaction.execute("UPDATE app_metadata SET value = ?2 WHERE key = 'active_project_path' AND value = ?1", params![path, root])?;
                } else {
                    transaction.execute(
                        "DELETE FROM app_metadata WHERE key = 'active_project_path' AND value = ?1",
                        [&path],
                    )?;
                }
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Close a project in the restored workspace without discarding its session metadata.
    pub fn close_project(
        &self,
        path: &str,
        next_active_project_path: Option<&str>,
    ) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        let path = path.to_owned();
        let next_active_project_path = next_active_project_path.map(str::to_owned);
        self.call(move |connection| {
            let path = &path;
            let next_active_project_path = next_active_project_path.as_deref();
            let transaction = connection.transaction()?;
            transaction.execute(
                "UPDATE projects SET is_open = 0 WHERE path = ?1 OR root_path = ?1",
                [path],
            )?;
            set_active_project_in_transaction(&transaction, next_active_project_path)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn set_active_project(&self, path: Option<&str>) -> Result<(), DatabaseError> {
        let path = path.map(str::to_owned);
        self.call(move |connection| {
            let path = path.as_deref();
            let transaction = connection.transaction()?;
            set_active_project_in_transaction(&transaction, path)?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn set_provider(&self, path: &str, provider: &str) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        validate_provider(provider)?;
        let path = path.to_owned();
        let provider = provider.to_owned();
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let changed = connection.execute(
                "UPDATE projects SET provider = ?2 WHERE path = ?1",
                params![path, provider],
            )?;
            if changed == 0 {
                return Err(DatabaseError::UnknownProject(path.to_owned()));
            }
            Ok(())
        })
    }

    pub fn select_session(
        &self,
        path: &str,
        provider: &str,
        session_id: Option<&str>,
    ) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        validate_provider(provider)?;
        if let Some(session_id) = session_id {
            validate_nonempty("session ID", session_id)?;
            if session_id.len() > 1024 {
                return Err(DatabaseError::InvalidValue("session ID is too long"));
            }
        }

        let path = path.to_owned();
        let provider = provider.to_owned();
        let session_id = session_id.map(str::to_owned);
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let session_id = session_id.as_deref();
            let transaction = connection.transaction()?;
            let exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE path = ?1)",
                [path],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(DatabaseError::UnknownProject(path.to_owned()));
            }

            transaction.execute(
                "INSERT INTO project_provider_state (project_path, provider, selected_session_id)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(project_path, provider) DO UPDATE SET
                     selected_session_id = excluded.selected_session_id",
                params![path, provider, session_id],
            )?;

            if let Some(session_id) = session_id {
                transaction.execute(
                    "INSERT INTO sessions (project_path, provider, session_id, first_seen_at, last_opened_at)
                     VALUES (?1, ?2, ?3, ?4, ?4)
                     ON CONFLICT(project_path, provider, session_id) DO UPDATE SET
                         last_opened_at = excluded.last_opened_at",
                    params![path, provider, session_id, unix_timestamp()],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Forget one archived session and clear its selection atomically.
    pub fn remove_session(
        &self,
        path: &str,
        provider: &str,
        session_id: &str,
    ) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        validate_provider(provider)?;
        validate_nonempty("session ID", session_id)?;
        let path = path.to_owned();
        let provider = provider.to_owned();
        let session_id = session_id.to_owned();
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let session_id = &session_id;
            let transaction = connection.transaction()?;
            transaction.execute(
                "DELETE FROM sessions WHERE project_path = ?1 AND provider = ?2 AND session_id = ?3",
                params![path, provider, session_id],
            )?;
            transaction.execute(
                "UPDATE project_provider_state SET selected_session_id = NULL WHERE project_path = ?1 AND provider = ?2 AND selected_session_id = ?3",
                params![path, provider, session_id],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Record provider-owned session identifiers without copying provider history or content.
    pub fn sync_sessions(
        &self,
        path: &str,
        provider: &str,
        session_ids: &[String],
    ) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        validate_provider(provider)?;
        for session_id in session_ids {
            validate_nonempty("session ID", session_id)?;
            if session_id.len() > 1024 {
                return Err(DatabaseError::InvalidValue("session ID is too long"));
            }
        }

        let path = path.to_owned();
        let provider = provider.to_owned();
        let session_ids = session_ids.to_vec();
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let session_ids = &session_ids;
            let transaction = connection.transaction()?;
            let exists = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE path = ?1)",
                [path],
                |row| row.get::<_, bool>(0),
            )?;
            if !exists {
                return Err(DatabaseError::UnknownProject(path.to_owned()));
            }

            let retained = session_ids
                .iter()
                .map(String::as_str)
                .collect::<HashSet<_>>();
            let stale_sessions = {
                let mut statement = transaction.prepare(
                    "SELECT session_id FROM sessions WHERE project_path = ?1 AND provider = ?2",
                )?;
                let rows =
                    statement.query_map(params![path, provider], |row| row.get::<_, String>(0))?;
                rows.collect::<Result<Vec<_>, _>>()?
            };
            for session_id in stale_sessions {
                if !retained.contains(session_id.as_str()) {
                    transaction.execute(
                        "DELETE FROM sessions
                         WHERE project_path = ?1 AND provider = ?2 AND session_id = ?3",
                        params![path, provider, session_id],
                    )?;
                }
            }

            let now = unix_timestamp();
            for session_id in session_ids {
                transaction.execute(
                    "INSERT INTO sessions (project_path, provider, session_id, first_seen_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(project_path, provider, session_id) DO NOTHING",
                    params![path, provider, session_id, now],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    /// Branches observed while a session executes survive listing, archive/undo,
    /// and app restarts. A stored NULL means an observed detached HEAD.
    pub fn session_branch(
        &self,
        path: &str,
        provider: &str,
        session_id: &str,
    ) -> Result<Option<SessionBranchHistory>, DatabaseError> {
        validate_branch_provider(provider)?;
        let path = path.to_owned();
        let provider = provider.to_owned();
        let session_id = session_id.to_owned();
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let session_id = &session_id;
            let branch: Option<Option<String>> = connection.query_row(
                "SELECT branch FROM session_branches WHERE project_path = ?1 AND provider = ?2 AND session_id = ?3",
                params![path, provider, session_id],
                |row| row.get(0),
            ).optional()?;
            let Some(branch) = branch else {
                return Ok(None);
            };
            let mut statement = connection.prepare(
                "SELECT branch FROM session_branch_history
                 WHERE project_path = ?1 AND provider = ?2 AND session_id = ?3 ORDER BY rowid",
            )?;
            let branches = statement
                .query_map(params![path, provider, session_id], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?;
            Ok(Some(SessionBranchHistory { branch, branches }))
        })
    }

    pub fn record_session_branch(
        &self,
        path: &str,
        provider: &str,
        session_id: &str,
        branch: Option<&str>,
    ) -> Result<(), DatabaseError> {
        validate_nonempty("project path", path)?;
        validate_branch_provider(provider)?;
        validate_nonempty("session ID", session_id)?;
        let path = path.to_owned();
        let provider = provider.to_owned();
        let session_id = session_id.to_owned();
        let branch = branch.map(str::to_owned);
        self.call(move |connection| {
            let path = &path;
            let provider = &provider;
            let session_id = &session_id;
            let branch = branch.as_deref();
            let transaction = connection.transaction()?;
            transaction.execute(
                "INSERT INTO session_branches (project_path, provider, session_id, branch)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(project_path, provider, session_id) DO UPDATE SET branch = excluded.branch",
                params![path, provider, session_id, branch],
            )?;
            if let Some(branch) = branch {
                transaction.execute(
                    "INSERT INTO session_branch_history (project_path, provider, session_id, branch)
                     VALUES (?1, ?2, ?3, ?4) ON CONFLICT DO NOTHING",
                    params![path, provider, session_id, branch],
                )?;
            }
            transaction.commit()?;
            Ok(())
        })
    }

    fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Connection) -> Result<T, DatabaseError> + Send + 'static,
    ) -> Result<T, DatabaseError> {
        self.connection
            .call(job)
            .ok_or(DatabaseError::WorkerClosed)?
    }
}

fn remember_project_in_transaction(
    transaction: &Transaction<'_>,
    project: &ProjectInfo,
) -> Result<(), DatabaseError> {
    validate_nonempty("project path", &project.path)?;
    validate_nonempty("project name", &project.name)?;
    transaction.execute(
        "UPDATE projects SET open_order = open_order + 1 WHERE is_open = 1",
        [],
    )?;
    transaction.execute(
        "INSERT INTO projects (path, name, is_open, open_order, last_opened_at)
         VALUES (?1, ?2, 1, 0, ?3)
         ON CONFLICT(path) DO UPDATE SET name = excluded.name, is_open = 1, open_order = 0, last_opened_at = excluded.last_opened_at",
        params![project.path, project.name, unix_timestamp()],
    )?;
    transaction.execute(
        "UPDATE projects SET is_open = 1 WHERE root_path = ?1",
        [&project.path],
    )?;
    Ok(())
}

fn set_active_project_in_transaction(
    transaction: &Transaction<'_>,
    path: Option<&str>,
) -> Result<(), DatabaseError> {
    if let Some(path) = path {
        validate_nonempty("project path", path)?;
        let is_open = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE path = ?1 AND is_open = 1)",
            [path],
            |row| row.get::<_, bool>(0),
        )?;
        if !is_open {
            return Err(DatabaseError::UnknownProject(path.to_owned()));
        }
        transaction.execute(
            "INSERT INTO app_metadata (key, value) VALUES ('active_project_path', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [path],
        )?;
        transaction.execute(
            "UPDATE projects SET selected_worktree_path = ?1
             WHERE path = COALESCE((SELECT root_path FROM projects WHERE path = ?1), ?1)",
            [path],
        )?;
    } else {
        transaction.execute(
            "DELETE FROM app_metadata WHERE key = 'active_project_path'",
            [],
        )?;
    }
    Ok(())
}

fn validate_provider(provider: &str) -> Result<(), DatabaseError> {
    if PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(DatabaseError::UnsupportedProvider(provider.to_owned()))
    }
}

fn validate_branch_provider(provider: &str) -> Result<(), DatabaseError> {
    if BRANCH_PROVIDERS.contains(&provider) {
        Ok(())
    } else {
        Err(DatabaseError::UnsupportedProvider(provider.to_owned()))
    }
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), DatabaseError> {
    if value.trim().is_empty() {
        Err(DatabaseError::InvalidValue(field))
    } else {
        Ok(())
    }
}

fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("could not create app data directory '{path}': {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("workspace database worker has stopped")]
    WorkerClosed,
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error("workspace database schema version {0} is newer than this application supports")]
    UnsupportedSchema(i64),
    #[error("workspace database contains an unsupported provider '{0}'")]
    UnsupportedProvider(String),
    #[error("project is not present in the workspace database: {0}")]
    UnknownProject(String),
    #[error("invalid workspace database value: {0}")]
    InvalidValue(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "caelum-database-test-{label}-{}-{}",
                std::process::id(),
                NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn database_path(&self) -> PathBuf {
            self.0.join("state").join("workspace.sqlite3")
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn project(path: &str, name: &str) -> ProjectInfo {
        ProjectInfo {
            name: name.to_owned(),
            path: path.to_owned(),
        }
    }

    #[test]
    fn fresh_database_migrates_and_can_be_opened_repeatedly() {
        let directory = TestDirectory::new("fresh");
        let database = WorkspaceDatabase::open(directory.database_path()).unwrap();
        assert!(database.restore().unwrap().projects.is_empty());
        drop(database);

        let database = WorkspaceDatabase::open(directory.database_path()).unwrap();
        assert!(database.restore().unwrap().projects.is_empty());
        let connection = Connection::open(directory.database_path()).unwrap();
        let version = connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn v5_branches_seed_scoped_history_and_keep_earlier_branches_after_restart() {
        let directory = TestDirectory::new("legacy-v5-branches");
        let path = directory.database_path();
        let database = WorkspaceDatabase::open(&path).unwrap();
        for (path, provider, branch) in [
            ("/work/one", "codex", Some("feature/legacy")),
            ("/work/one", "opencode", None),
            ("/work/two", "codex", Some("feature/other")),
        ] {
            database
                .record_session_branch(path, provider, "same-id", branch)
                .unwrap();
        }
        database
            .call(|connection| {
                connection
                    .execute_batch("DROP TABLE session_branch_history; PRAGMA user_version = 5;")?;
                Ok(())
            })
            .unwrap();
        drop(database);

        let database = WorkspaceDatabase::open(&path).unwrap();
        assert_eq!(
            database
                .session_branch("/work/one", "codex", "same-id")
                .unwrap(),
            Some(SessionBranchHistory {
                branch: Some("feature/legacy".into()),
                branches: vec!["feature/legacy".into()],
            })
        );
        for branch in [Some("feature/second"), Some("feature/legacy"), None] {
            database
                .record_session_branch("/work/one", "codex", "same-id", branch)
                .unwrap();
        }
        drop(database);

        let database = WorkspaceDatabase::open(&path).unwrap();
        assert_eq!(
            database
                .session_branch("/work/one", "codex", "same-id")
                .unwrap(),
            Some(SessionBranchHistory {
                branch: None,
                branches: vec!["feature/legacy".into(), "feature/second".into()],
            })
        );
        assert_eq!(
            database
                .session_branch("/work/one", "opencode", "same-id")
                .unwrap(),
            Some(SessionBranchHistory {
                branch: None,
                branches: vec![]
            })
        );
        assert_eq!(
            database
                .session_branch("/work/two", "codex", "same-id")
                .unwrap(),
            Some(SessionBranchHistory {
                branch: Some("feature/other".into()),
                branches: vec!["feature/other".into()],
            })
        );
        assert_eq!(
            database
                .session_branch("/work/one", "codex", "unknown")
                .unwrap(),
            None
        );
    }

    #[test]
    fn reports_unavailable_storage_without_panicking() {
        let directory = TestDirectory::new("unavailable");
        let blocking_file = directory.0.join("not-a-directory");
        fs::write(&blocking_file, "occupied").unwrap();

        let result = WorkspaceDatabase::open(blocking_file.join("workspace.sqlite3"));
        assert!(matches!(result, Err(DatabaseError::CreateDirectory { .. })));
    }

    #[test]
    fn version_one_database_upgrades_without_losing_projects() {
        let directory = TestDirectory::new("upgrade");
        let path = directory.database_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE projects (
                    path TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    is_open INTEGER NOT NULL DEFAULT 1 CHECK (is_open IN (0, 1)),
                    open_order INTEGER NOT NULL DEFAULT 0,
                    last_opened_at INTEGER NOT NULL,
                    provider TEXT NOT NULL DEFAULT 'codex' CHECK (provider IN ('codex', 'opencode'))
                 );
                 INSERT INTO projects (path, name, is_open, open_order, last_opened_at, provider)
                    VALUES ('/work/kept', 'kept', 1, 0, 10, 'opencode');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let database = WorkspaceDatabase::open(&path).unwrap();
        let snapshot = database.restore().unwrap();
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.projects[0].project.path, "/work/kept");
        assert_eq!(snapshot.projects[0].provider, "opencode");
        assert_eq!(snapshot.projects[0].opencode_session_id, None);
    }

    #[test]
    fn legacy_v2_session_timestamps_migrate_without_losing_metadata() {
        let directory = TestDirectory::new("legacy-v2");
        let path = directory.database_path();
        let database = WorkspaceDatabase::open(&path).unwrap();
        database
            .remember_project(&project("/work/kept", "kept"))
            .unwrap();
        database
            .select_session("/work/kept", "opencode", Some("ses_kept"))
            .unwrap();
        database.set_active_project(Some("/work/kept")).unwrap();
        let legacy_schema = "DROP TABLE session_branch_history;
             DROP TABLE session_branches;
             DROP TABLE sessions;
             CREATE TABLE sessions (
                project_path TEXT NOT NULL REFERENCES projects(path) ON DELETE CASCADE,
                provider TEXT NOT NULL CHECK (provider IN ('codex', 'opencode')),
                session_id TEXT NOT NULL,
                first_seen_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL,
                local_label TEXT,
                PRIMARY KEY (project_path, provider, session_id)
             );
             CREATE INDEX sessions_recent_idx ON sessions(project_path, provider, last_opened_at DESC);
             INSERT INTO sessions VALUES ('/work/kept', 'opencode', 'ses_kept', 10, 20, 'Keep this label');
             DROP INDEX project_root_idx;
             ALTER TABLE projects DROP COLUMN root_path;
             ALTER TABLE projects DROP COLUMN selected_worktree_path;
             PRAGMA user_version = 2;";
        database
            .call(move |connection| {
                connection.execute_batch(legacy_schema)?;
                Ok(())
            })
            .unwrap();
        assert!(database
            .sync_sessions(
                "/work/kept",
                "opencode",
                &["ses_kept".into(), "ses_new".into()]
            )
            .is_err());
        drop(database);

        let database = WorkspaceDatabase::open(&path).unwrap();
        database
            .sync_sessions(
                "/work/kept",
                "opencode",
                &["ses_kept".into(), "ses_new".into()],
            )
            .unwrap();
        let snapshot = database.restore().unwrap();
        assert_eq!(snapshot.active_project_path.as_deref(), Some("/work/kept"));
        assert_eq!(
            snapshot.projects[0].opencode_session_id.as_deref(),
            Some("ses_kept")
        );
        let kept: (i64, i64, String) = Connection::open(&path).unwrap().query_row(
            "SELECT first_seen_at, last_opened_at, local_label FROM sessions WHERE session_id = 'ses_kept'", [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(kept, (10, 20, "Keep this label".into()));
        let opened: Option<i64> = Connection::open(&path)
            .unwrap()
            .query_row(
                "SELECT last_opened_at FROM sessions WHERE session_id = 'ses_new'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(opened, None);
        database
            .select_session("/work/kept", "opencode", Some("ses_new"))
            .unwrap();
        drop(database);
        let reopened = WorkspaceDatabase::open(&path).unwrap();
        assert_eq!(
            reopened.restore().unwrap().projects[0]
                .opencode_session_id
                .as_deref(),
            Some("ses_new")
        );
    }

    #[test]
    fn restores_open_projects_providers_and_selected_session_ids() {
        let directory = TestDirectory::new("round-trip");
        let database = WorkspaceDatabase::open(directory.database_path()).unwrap();
        database
            .remember_project(&project("/work/one", "one"))
            .unwrap();
        database
            .remember_project(&project("/work/two", "two"))
            .unwrap();
        database.set_provider("/work/one", "opencode").unwrap();
        database
            .select_session("/work/one", "codex", Some("thread-1"))
            .unwrap();
        database
            .select_session("/work/one", "opencode", Some("session-1"))
            .unwrap();
        database.set_active_project(Some("/work/one")).unwrap();

        let snapshot = database.restore().unwrap();
        assert_eq!(snapshot.active_project_path.as_deref(), Some("/work/one"));
        assert_eq!(snapshot.projects[0].project.path, "/work/two");
        let first = snapshot
            .projects
            .iter()
            .find(|entry| entry.project.path == "/work/one")
            .unwrap();
        assert_eq!(first.provider, "opencode");
        assert_eq!(first.codex_session_id.as_deref(), Some("thread-1"));
        assert_eq!(first.opencode_session_id.as_deref(), Some("session-1"));
        assert_eq!(first.codex_session_ids, ["thread-1"]);
        assert_eq!(first.opencode_session_ids, ["session-1"]);

        let connection = Connection::open(directory.database_path()).unwrap();
        let history_table_exists = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'conversation_history')",
                [],
                |row| row.get::<_, bool>(0),
            )
            .unwrap();
        assert!(!history_table_exists);
    }

    #[test]
    fn session_branches_accept_antigravity_provider() {
        let directory = TestDirectory::new("antigravity-branch");
        let path = directory.database_path();
        let database = WorkspaceDatabase::open(&path).unwrap();
        database
            .record_session_branch("/work/one", "antigravity", "acp-1", Some("feat/acp"))
            .unwrap();
        drop(database);

        let reopened = WorkspaceDatabase::open(&path).unwrap();
        assert_eq!(
            reopened
                .session_branch("/work/one", "antigravity", "acp-1")
                .unwrap(),
            Some(SessionBranchHistory {
                branch: Some("feat/acp".into()),
                branches: vec!["feat/acp".into()],
            })
        );
    }
}
