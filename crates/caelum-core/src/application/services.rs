//! Scoped files, checkout and head-pinned review coordination. Renderers never
//! own subprocesses, pagination or response admission.
mod completion;
mod requests;
mod review;
mod review_cache;

pub use review::Review;

use super::{Preferences, ProviderRuntime};
use crate::{files, git::GitClient, github};
use caelum_protocol::*;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Task {
    Shell {
        project: String,
        session: String,
        id: String,
        stop: bool,
    },
    #[serde(skip)]
    Reveal {
        project: String,
        path: String,
    },
    EncodeImage {
        project: String,
        bytes: Vec<u8>,
    },
    #[serde(skip)]
    Updates,
    #[serde(skip)]
    Update(crate::updates::Job),
    #[serde(skip)]
    HostStats,
    #[serde(skip)]
    InstalledProviders,
    Image {
        project: String,
        source: String,
    },
    Usage {
        project: String,
        #[serde(default = "default_usage_provider")]
        provider: super::Provider,
    },
    Stats {
        project: String,
        filter: OpenCodeStatsFilter,
    },
    AttachPullRequest {
        project: String,
        url: String,
    },
    Completions {
        project: String,
        query: String,
    },
    Directory {
        project: String,
        path: String,
        offset: usize,
    },
    File {
        project: String,
        path: String,
    },
    Search {
        project: String,
        query: String,
        mode: ProjectSearchMode,
    },
    Git {
        project: String,
    },
    Diff {
        project: String,
        path: String,
    },
    Worktrees {
        project: String,
        directory: String,
    },
    CreateWorktree {
        project: String,
        branch: String,
        base: String,
        directory: String,
    },
    RemoveWorktree {
        project: String,
        path: String,
    },
    RenameWorktree {
        project: String,
        path: String,
        name: String,
        directory: String,
    },
    Discover {
        project: String,
        branch: String,
    },
    Details {
        project: String,
        url: String,
    },
    ReviewCache {
        project: String,
        url: String,
    },
    ReviewFiles {
        project: String,
        url: String,
        head: String,
        base: String,
        page: u32,
    },
    Feedback {
        project: String,
        url: String,
        head: String,
        kind: GitHubFeedbackKind,
        cursor: Option<String>,
        thread: Option<String>,
    },
    Merge {
        project: String,
        url: String,
        head: String,
        method: GitHubMergeMethod,
    },
    SetAutoMerge {
        project: String,
        url: String,
        head: String,
        /// `Some(method)` enables auto-merge; `None` disables it.
        method: Option<GitHubMergeMethod>,
    },
    UpdateBranch {
        project: String,
        url: String,
        head: String,
    },
    #[serde(skip)]
    ImportSettings {
        path: String,
    },
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum TaskReply {
    Shell(String),
    #[serde(skip)]
    Updates(UpdateCheck),
    #[serde(skip)]
    Update(crate::updates::Reply),
    #[serde(skip)]
    HostStats(crate::host_stats::HostSample),
    #[serde(skip)]
    InstalledProviders(Vec<super::Provider>),
    Image(String),
    Usage(serde_json::Value),
    Stats(OpenCodeStats, Vec<OpenCodeStatsProject>),
    Directory(ProjectDirectoryListing),
    File(ProjectFileContent),
    Search(ProjectSearchResults),
    Git(GitRepositoryStatus, Vec<SessionBranchUpdate>),
    Diff(GitFileDiff),
    Worktrees(Option<GitWorktrees>),
    Worktree(GitWorktree),
    Discovered(Option<GitHubPullRequest>),
    Details(Box<GitHubPullRequestDetails>),
    ReviewCache(Option<Box<github::CachedReview>>),
    ReviewFiles(GitHubPullRequestFiles),
    Feedback(GitHubPullRequestFeedback),
    AutoMerge(GitHubAutoMergeResult),
    #[serde(skip)]
    Preferences(Preferences),
    Done,
}

macro_rules! project_field {
    ($task:expr) => {
        match $task {
            Task::Shell { project, .. }
            | Task::Completions { project, .. }
            | Task::Image { project, .. }
            | Task::EncodeImage { project, .. }
            | Task::Reveal { project, .. }
            | Task::Usage { project, .. }
            | Task::Stats { project, .. }
            | Task::AttachPullRequest { project, .. }
            | Task::Directory { project, .. }
            | Task::File { project, .. }
            | Task::Search { project, .. }
            | Task::Git { project }
            | Task::Diff { project, .. }
            | Task::Worktrees { project, .. }
            | Task::CreateWorktree { project, .. }
            | Task::RemoveWorktree { project, .. }
            | Task::RenameWorktree { project, .. }
            | Task::Discover { project, .. }
            | Task::Details { project, .. }
            | Task::ReviewCache { project, .. }
            | Task::ReviewFiles { project, .. }
            | Task::Feedback { project, .. }
            | Task::Merge { project, .. }
            | Task::SetAutoMerge { project, .. }
            | Task::UpdateBranch { project, .. } => Some(project),
            Task::ImportSettings { .. }
            | Task::Updates
            | Task::Update(_)
            | Task::HostStats
            | Task::InstalledProviders => None,
        }
    };
}

impl Task {
    fn mutates_review(&self) -> bool {
        matches!(
            self,
            Self::Merge { .. } | Self::SetAutoMerge { .. } | Self::UpdateBranch { .. }
        )
    }
    pub fn project(&self) -> &str {
        project_field!(self).map_or("", String::as_str)
    }
    pub(crate) fn project_mut(&mut self) -> Option<&mut String> {
        project_field!(self)
    }
    pub fn review(&self) -> Option<(&str, Option<&str>)> {
        match self {
            Self::Details { url, .. } | Self::ReviewCache { url, .. } => Some((url, None)),
            Self::ReviewFiles { url, head, .. }
            | Self::Feedback { url, head, .. }
            | Self::Merge { url, head, .. }
            | Self::SetAutoMerge { url, head, .. }
            | Self::UpdateBranch { url, head, .. } => Some((url, Some(head))),
            _ => None,
        }
    }
    pub(super) fn slot(&self) -> String {
        let part = match self {
            Self::Shell {
                session, id, stop, ..
            } => format!("shell:{session}:{id}:{stop}"),
            Self::Updates => "updates".into(),
            Self::Update(job) => format!("update-installation:{}", job.id),
            Self::HostStats => "host-stats".into(),
            Self::InstalledProviders => "installed-providers".into(),
            Self::Reveal { path, .. } => format!("reveal:{path}"),
            Self::EncodeImage { .. } => "encode-image".into(),
            Self::Image { source, .. } => format!("image:{source}"),
            Self::Usage { provider, .. } => format!("usage:{}", provider.as_str()),
            Self::Stats { .. } => "stats".into(),
            Self::AttachPullRequest { url, .. } => format!("attach-pr:{url}"),
            Self::Completions { .. } => "completions".into(),
            Self::Directory { path, .. } => format!("directory:{path}"),
            Self::File { path, .. } => format!("file:{path}"),
            Self::Search { .. } => "search".into(),
            Self::Git { .. } => "git".into(),
            Self::Diff { path, .. } => format!("diff:{path}"),
            Self::Worktrees { .. } => "worktrees".into(),
            Self::CreateWorktree { .. } => "worktree-create".into(),
            Self::RemoveWorktree { path, .. } => format!("worktree-remove:{path}"),
            Self::RenameWorktree { path, .. } => format!("worktree-rename:{path}"),
            Self::Discover { branch, .. } => format!("discover:{branch}"),
            Self::Details { url, .. } => format!("details:{url}"),
            Self::ReviewCache { url, .. } => format!("review-cache:{url}"),
            Self::ReviewFiles { url, .. } => format!("files:{url}"),
            Self::Feedback {
                url, kind, thread, ..
            } => format!("feedback:{url}:{kind:?}:{thread:?}"),
            Self::Merge { url, .. }
            | Self::SetAutoMerge { url, .. }
            | Self::UpdateBranch { url, .. } => format!("mutation:{url}"),
            Self::ImportSettings { .. } => "settings".into(),
        };
        serde_json::json!([self.project(), part]).to_string()
    }

    pub(super) fn execute(&self, runtime: &ProviderRuntime) -> Result<TaskReply, String> {
        let worktrees = &runtime.worktrees;
        let git = GitClient::new();
        macro_rules! reply {
            ($call:expr, $variant:ident) => {
                $call.map(TaskReply::$variant).map_err(|e| e.to_string())
            };
        }
        match self {
            Self::Shell {
                project,
                session,
                id,
                stop,
            } => reply!(
                runtime.opencode.shell_action(project, session, id, *stop),
                Shell
            ),
            Self::Reveal { project, path } => {
                let project = crate::projects::Project::open(project).map_err(|e| e.to_string())?;
                let path = project
                    .resolve_scoped_path(path)
                    .map_err(|e| e.to_string())?;
                let status = std::process::Command::new(if cfg!(target_os = "macos") {
                    "open"
                } else {
                    "xdg-open"
                })
                .args(if cfg!(target_os = "macos") {
                    vec!["-R".as_ref(), path.as_os_str()]
                } else {
                    vec![path.as_os_str()]
                })
                .status()
                .map_err(|e| e.to_string())?;
                if !status.success() {
                    return Err("Unable to reveal this path".into());
                }
                Ok(TaskReply::Done)
            }
            Self::EncodeImage { bytes, .. } => {
                crate::images::data_url(bytes.as_slice()).map(TaskReply::Image)
            }
            Self::Image { project, source } => {
                let data = if source.starts_with("data:image/") {
                    source.clone()
                } else if let Some(hash) = source.strip_prefix("opencode-media:") {
                    runtime
                        .opencode
                        .read_image(project, hash)
                        .map_err(|e| e.to_string())?
                } else if source.starts_with("https://") || source.starts_with("http://") {
                    let response = reqwest::blocking::Client::builder()
                        .timeout(std::time::Duration::from_secs(20))
                        .build()
                        .map_err(|e| e.to_string())?
                        .get(source)
                        .send()
                        .and_then(|r| r.error_for_status())
                        .map_err(|e| e.to_string())?;
                    crate::images::data_url(response)?
                } else {
                    files::read_image(project, source)?
                };
                Ok(TaskReply::Image(data))
            }
            Self::Updates => reply!(
                crate::updates::check_for_updates(env!("CARGO_PKG_VERSION")),
                Updates
            ),
            Self::Update(job) => runtime.updates.execute(job).map(TaskReply::Update),
            Self::HostStats => crate::host_stats::sample().map(TaskReply::HostStats),
            Self::InstalledProviders => {
                Ok(TaskReply::InstalledProviders(runtime.installed_providers()))
            }
            Self::Usage { project, provider } => match provider {
                super::Provider::Antigravity => {
                    reply!(runtime.antigravity.usage_limits(project), Usage)
                }
                _ => reply!(runtime.opencode.usage_limits(project), Usage),
            },
            Self::Stats { project, filter } => Ok(TaskReply::Stats(
                runtime
                    .opencode
                    .session_stats(project, filter)
                    .map_err(|e| e.to_string())?,
                runtime
                    .opencode
                    .stats_projects(project)
                    .map_err(|e| e.to_string())?,
            )),
            Self::AttachPullRequest { project, url } => {
                reply!(
                    github::pull_request_details(project, url).map(Box::new),
                    Details
                )
            }
            Self::Completions { project, query } => reply!(
                files::search_project(project, query, ProjectSearchMode::Files, false),
                Search
            ),
            Self::Directory {
                project,
                path,
                offset,
            } => reply!(
                files::list_directory(project, path, *offset, 200),
                Directory
            ),
            Self::File { project, path } => reply!(files::read_text_file(project, path), File),
            Self::Search {
                project,
                query,
                mode,
            } => reply!(files::search_project(project, query, *mode, false), Search),
            Self::Git { project } => {
                let status = git.repository_status(project).map_err(|e| e.to_string())?;
                let branches = runtime.branches.refresh_checkout(&status);
                Ok(TaskReply::Git(status, branches))
            }
            Self::Diff { project, path } => reply!(git.file_diff(project, path), Diff),
            Self::Worktrees { project, directory } => {
                reply!(worktrees.list(project, directory), Worktrees)
            }
            Self::CreateWorktree {
                project,
                branch,
                base,
                directory,
            } => reply!(worktrees.create(project, branch, base, directory), Worktree),
            Self::RemoveWorktree { project, path } => {
                runtime
                    .codex
                    .ensure_checkout_idle(std::path::Path::new(path))
                    .map_err(|e| e.to_string())?;
                runtime
                    .opencode
                    .ensure_checkout_idle(std::path::Path::new(path))
                    .map_err(|e| e.to_string())?;
                let removed = worktrees.remove(project, path).map_err(|e| e.to_string())?;
                runtime.codex.release_checkout(std::path::Path::new(path));
                runtime
                    .opencode
                    .release_checkout(std::path::Path::new(path));
                Ok(TaskReply::Worktree(removed))
            }
            Self::RenameWorktree {
                project,
                path,
                name,
                ..
            } => reply!(worktrees.rename(project, path, name), Worktree),
            Self::Discover { project, branch } => {
                reply!(github::open_pull_request(project, branch), Discovered)
            }
            Self::Details { project, url } => review_cache::details(runtime, project, url)
                .map(Box::new)
                .map(TaskReply::Details),
            Self::ReviewCache { url, .. } => Ok(TaskReply::ReviewCache(
                review_cache::cache(runtime)
                    .and_then(|cache| cache.load(url))
                    .map(Box::new),
            )),
            Self::ReviewFiles {
                project,
                url,
                head,
                base,
                page,
            } => review_cache::files(runtime, project, url, head, base, *page)
                .map(TaskReply::ReviewFiles),
            Self::Feedback {
                project,
                url,
                head,
                kind,
                cursor,
                thread,
            } => reply!(
                github::pull_request_feedback(
                    project,
                    url,
                    head,
                    *kind,
                    cursor.as_deref(),
                    thread.as_deref()
                ),
                Feedback
            ),
            Self::Merge {
                project,
                url,
                head,
                method,
            } => github::merge_pull_request(project, url, *method, head)
                .map(|()| TaskReply::Done)
                .map_err(|e| e.to_string()),
            Self::SetAutoMerge {
                project,
                url,
                head,
                method,
            } => reply!(
                github::set_pull_request_auto_merge(project, url, head, *method),
                AutoMerge
            ),
            Self::UpdateBranch { project, url, head } => {
                github::update_pull_request_branch(project, url, head)
                    .map(|()| TaskReply::Done)
                    .map_err(|e| e.to_string())
            }
            Self::ImportSettings { path } => Preferences::import_tauri(
                &std::fs::read_to_string(path).map_err(|e| e.to_string())?,
            )
            .map(TaskReply::Preferences),
        }
    }
}

fn default_usage_provider() -> super::Provider {
    super::Provider::OpenCode
}

#[derive(Default)]
pub struct Checkout {
    pub shell_output: BTreeMap<(String, String), String>,
    pub usage: HashMap<super::Provider, serde_json::Value>,
    pub stats: Option<OpenCodeStats>,
    pub stats_projects: Vec<OpenCodeStatsProject>,
    pub stats_filter: String,
    pub completions: Option<ProjectSearchResults>,
    pub completion_query: String,
    pub directories: BTreeMap<String, ProjectDirectoryListing>,
    pub files: BTreeMap<String, ProjectFileContent>,
    pub tabs: Vec<String>,
    pub selected_file: Option<String>,
    pub search: Option<ProjectSearchResults>,
    pub search_query: String,
    pub git: Option<GitRepositoryStatus>,
    pub diffs: BTreeMap<String, GitFileDiff>,
    pub worktrees: Option<GitWorktrees>,
    worktrees_revision: u64,
    pub pull_requests: Vec<GitHubPullRequest>,
    pub error: Option<String>,
}

struct Discovered {
    url: String,
    request: u64,
}

#[derive(Default)]
pub struct Services {
    pub images: BTreeMap<(String, String), String>,
    pub revision: u64,
    requests: HashMap<String, requests::Slot>,
    pub update: Option<UpdateCheck>,
    pub updater: crate::updates::State,
    pub host: Option<crate::host_stats::HostHistory>,
    pub installed_providers: Option<Vec<super::Provider>>,
    pub checkouts: BTreeMap<String, Checkout>,
    pub reviews: BTreeMap<(String, String), Review>,
    discovered: HashMap<(String, String), Option<Discovered>>,
    merged_pull_requests: HashSet<String>,
}

impl Services {
    pub fn record_discovered_pull_request(
        &mut self,
        project: impl Into<String>,
        branch: impl Into<String>,
        url: impl Into<String>,
    ) {
        self.discovered.insert(
            (project.into(), branch.into()),
            Some(Discovered {
                url: url.into(),
                request: 0,
            }),
        );
    }

    pub(super) fn branch_pull_request(
        &self,
        project: &str,
        branch: &str,
    ) -> Option<&GitHubPullRequest> {
        let found = self
            .discovered
            .get(&(project.into(), branch.into()))?
            .as_ref()?;

        let discovered = self
            .checkouts
            .get(project)?
            .pull_requests
            .iter()
            .find(|pr| pr.url == found.url)?;
        let current = self
            .reviews
            .get(&(project.into(), found.url.clone()))
            .filter(|r| r.newer_than(found.request));
        if let Some(review) = current {
            let details = review.details()?;
            if details.state != "OPEN" || details.head_ref_name != branch {
                return None;
            }
            Some(&details.pull_request)
        } else {
            Some(discovered)
        }
    }

    pub(super) fn requested_changes(
        &self,
        project: &str,
        branch: &str,
    ) -> Option<&GitHubPullRequest> {
        let pr = self.branch_pull_request(project, branch)?;
        (pr.review_decision.as_deref() == Some("CHANGES_REQUESTED")).then_some(pr)
    }

    pub(super) fn pull_request_branches<'a>(
        &'a self,
        project: &'a str,
    ) -> impl Iterator<Item = String> + 'a {
        self.discovered
            .keys()
            .filter(move |(path, _)| path == project)
            .map(|(_, branch)| branch.clone())
    }

    /// A listing read from any checkout describes the same repository. Prefer
    /// the newest successful request, even if replies finish out of order.
    pub fn worktrees(&self, project: &str) -> Option<&GitWorktrees> {
        self.checkouts
            .values()
            .filter_map(|checkout| {
                let listing = checkout.worktrees.as_ref()?;
                (listing.project.path == project
                    || listing
                        .worktrees
                        .iter()
                        .any(|tree| tree.project.path == project))
                .then_some((checkout.worktrees_revision, listing))
            })
            .max_by_key(|(revision, _)| *revision)
            .map(|(_, listing)| listing)
    }

    pub fn refresh_review(&mut self, project: &str, url: &str) {
        self.revision += 1;
        let key = (project.into(), url.into());
        let review = self.reviews.entry(key.clone()).or_default();
        review.generation += 1;
        review.clear_files();
        review.clear_feedback();
        self.invalidate_review_reads(&key);
    }
    pub(super) fn start(&mut self, task: &Task, id: u64) {
        if let Task::SetAutoMerge { project, url, .. } = task {
            self.invalidate_details(project, url);
            self.reviews
                .entry((project.clone(), url.clone()))
                .or_default()
                .notice = None;
        }
        if let Task::Stats { filter, .. } = task {
            let checkout = self.checkouts.entry(task.project().into()).or_default();
            let key = serde_json::to_string(filter).unwrap_or_default();
            if checkout.stats_filter != key {
                checkout.stats = None;
                checkout.stats_filter = key;
            }
        }
        self.start_slot(task, id);
        self.checkouts
            .entry(task.project().into())
            .or_default()
            .error = None;
    }

    fn invalidate_details(&mut self, project: &str, url: &str) {
        let slot = Task::Details {
            project: project.into(),
            url: url.into(),
        }
        .slot();
        self.requests.remove(&slot);
    }
}
