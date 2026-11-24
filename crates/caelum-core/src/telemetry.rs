//! Local completed-turn accounting. The writer accepts bounded numeric records,
//! never conversation content. Queries run behind already accepted writes.
use caelum_protocol::{OpenCodeStats, OpenCodeStatsFilter, OpenCodeStatsProject, StatsProvider};
use rusqlite::{params, Connection};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
};

mod meter;
mod query;
#[cfg(test)]
mod tests;
mod turns;

use meter::Meter;
pub(crate) use turns::boat;
pub(crate) use turns::Tracking;

type Report = (OpenCodeStats, Vec<OpenCodeStatsProject>);

#[derive(Clone, Debug)]
pub struct Record {
    project: String,
    provider: StatsProvider,
    session: String,
    turn: String,
    model: String,
    timestamp: u64,
    duration: u64,
    meter: Meter,
    estimated: bool,
}

impl Record {
    fn valid(&self) -> bool {
        !self.project.is_empty()
            && self.project.len() <= 8192
            && [&self.session, &self.turn, &self.model]
                .iter()
                .all(|s| !s.is_empty() && s.len() <= 512)
    }
    fn insert(&self, db: &Connection) -> rusqlite::Result<usize> {
        let m = &self.meter;
        db.execute("INSERT INTO turns (project_path,provider,session_id,turn_id,model,timestamp,duration_ms,input_tokens,output_tokens,cached_tokens,cache_write_tokens,reasoning_tokens,estimated_cost_usd,estimated_tokens)
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
           ON CONFLICT(project_path,provider,session_id,turn_id) DO NOTHING",params![
            self.project,self.provider.name(),self.session,self.turn,self.model,self.timestamp,self.duration,
            m.input,m.output,m.read,m.write,m.reasoning,m.cost.or_else(|| m.price(&self.model)),self.estimated
        ])
    }
}

#[derive(Clone, Default)]
pub(crate) struct Store {
    jobs: Arc<OnceLock<crate::blocking_actor::BlockingActor<Result<Connection, String>>>>,
    dropped: Arc<AtomicU64>,
}

impl Store {
    pub(crate) fn open(&self, workspace: &Path) {
        self.jobs.get_or_init(|| {
            let actor = crate::blocking_actor::BlockingActor::spawn(
                "caelum-turn-usage",
                Err("Usage storage is unavailable.".into()),
            );
            let path = workspace.with_file_name("usage-v1.sqlite3");
            actor.try_post(move |db| *db = open(&path).map_err(|e| e.to_string()));
            actor
        });
    }

    pub(crate) fn record(&self, row: Record) {
        let dropped = self.dropped.clone();
        if !row.valid()
            || !self.jobs.get().is_some_and(|actor| {
                actor.try_post(move |db| {
                    if db.as_ref().map_or(true, |db| row.insert(db).is_err()) {
                        dropped.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn read(&self, filter: &OpenCodeStatsFilter, server: bool) -> Result<Report, String> {
        let tx = self
            .jobs
            .get()
            .ok_or("Local usage storage is unavailable.")?;
        let filter = filter.clone();
        let (mut stats, projects) = tx
            .call(move |db| {
                db.as_ref()
                    .map_err(Clone::clone)
                    .and_then(|db| query::read(db, &filter, server).map_err(|e| e.to_string()))
            })
            .ok_or("Usage storage closed.")??;
        stats.dropped = self.dropped.load(Ordering::Relaxed);
        Ok((stats, projects))
    }

    pub(crate) fn stats(
        &self,
        server: Option<(&crate::opencode::OpenCodeProvider, &str)>,
        filter: &OpenCodeStatsFilter,
    ) -> Result<Report, String> {
        if matches!((filter.from,filter.to),(Some(a),Some(b)) if a>=b) {
            return Err("Statistics end must be after start.".into());
        }
        let include = filter.provider.is_none_or(|p| p == StatsProvider::OpenCode);
        let mut catalogue = Vec::new();
        let mut authoritative = None;
        if let Some((provider, project)) = server.filter(|_| include) {
            if let Ok(projects) = provider.stats_projects(project) {
                let mut f = filter.clone();
                f.project = filter.project.as_ref().and_then(|path| {
                    projects
                        .iter()
                        .find(|p| &p.canonical == path)
                        .map(|p| p.id.clone())
                });
                if filter.project.is_none() || f.project.is_some() {
                    f.timezone = Some("UTC".into());
                    authoritative = provider.session_stats(project, &f).ok();
                }
                catalogue = projects;
            }
        }
        let (mut stats, local) = self.read(filter, authoritative.is_some())?;
        stats.recorded_only = include && authoritative.is_none();
        if let Some(history) = authoritative {
            query::merge(&mut stats, history);
        }
        let mut projects = std::collections::BTreeMap::new();
        for mut p in local.into_iter().chain(catalogue) {
            p.id.clone_from(&p.canonical);
            projects.insert(p.id.clone(), p);
        }
        Ok((stats, projects.into_values().collect()))
    }
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS turns (
            project_path TEXT NOT NULL,provider TEXT NOT NULL,session_id TEXT NOT NULL,turn_id TEXT NOT NULL,model TEXT NOT NULL,
            timestamp INTEGER NOT NULL,duration_ms INTEGER NOT NULL,input_tokens INTEGER NOT NULL,output_tokens INTEGER NOT NULL,
            cached_tokens INTEGER NOT NULL,cache_write_tokens INTEGER NOT NULL,reasoning_tokens INTEGER NOT NULL,estimated_cost_usd REAL,
            estimated_tokens INTEGER NOT NULL,PRIMARY KEY(project_path,provider,session_id,turn_id)
        ) WITHOUT ROWID;
        CREATE INDEX IF NOT EXISTS turns_by_time ON turns(timestamp);
        CREATE INDEX IF NOT EXISTS turns_by_provider ON turns(provider,timestamp);")?;
    Ok(db)
}
