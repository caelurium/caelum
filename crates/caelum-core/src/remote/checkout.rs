use super::{Machine, MachineId};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Instant};

/// The only local input is the published GitHub origin. Remote paths are never passed to Project::open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub project: String,
    pub account: String,
    pub repository: String,
    pub base: String,
    pub environment: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    Small,
    #[default]
    Standard,
    Large,
    Xlarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub token: String,
    pub branch: String,
    pub size: Size,
    pub minutes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Create(Plan),
    Prepare {
        machine: Machine,
        new_branch: Option<String>,
    },
    Ready {
        machine: Machine,
        branch: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkout {
    pub source: Source,
    pub phase: Phase,
    #[serde(default)]
    pub thread: super::Thread,
}

impl Checkout {
    pub fn machine(&self) -> Option<&Machine> {
        match &self.phase {
            Phase::Create(_) => None,
            Phase::Prepare { machine, .. } | Phase::Ready { machine, .. } => Some(machine),
        }
    }
    pub fn title(&self) -> &str {
        self.machine().map_or("Creating machine", |m| &m.name)
    }
    pub fn target(&self) -> Option<Target> {
        Some(Target {
            machine: self.machine()?.key.clone(),
            repository: self.source.repository.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub machine: MachineId,
    pub repository: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub checkouts: BTreeMap<String, Checkout>,
    pub selected: Option<String>,
}

#[derive(Debug)]
pub enum Job {
    Busy(u64),
    Wait(Instant),
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum Picker {
    Loading { request: u64, project: String },
    Ready(Source),
    Failed { project: String, error: String },
}

pub fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value != "HEAD"
        && !value.starts_with('-')
        && !value.contains("..")
        && !value.ends_with('.')
        && value
            .split('/')
            .all(|p| !p.is_empty() && !p.starts_with('.') && !p.ends_with(".lock"))
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/-_.".contains(&b))
}

pub(super) fn repo_name(slug: &str) -> Result<&str, String> {
    let (owner, repo) = slug
        .split_once('/')
        .ok_or("Expected a GitHub owner/repository")?;
    for part in [owner, repo] {
        if part.is_empty()
            || part.starts_with('.')
            || !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err("Invalid GitHub repository name".into());
        }
    }
    Ok(repo)
}

pub(crate) fn github_slug(url: &str) -> Option<String> {
    let path = if let Some(path) = url.trim().strip_prefix("git@github.com:") {
        path.to_owned()
    } else {
        let url = reqwest::Url::parse(url.trim()).ok()?;
        if url.host_str()? != "github.com" || !matches!(url.scheme(), "ssh" | "https") {
            return None;
        }
        url.path().trim_start_matches('/').to_owned()
    };
    let path = path.trim_end_matches('/');
    let slug = path.strip_suffix(".git").unwrap_or(path);
    repo_name(slug).ok()?;
    Some(slug.to_owned())
}
