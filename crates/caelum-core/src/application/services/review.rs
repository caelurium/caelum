use super::*;

#[derive(Default)]
enum State {
    #[default]
    Unloaded,
    Verified(Box<Verified>),
}

struct Verified {
    source: Source,
    details: GitHubPullRequestDetails,
    files: Option<Files>,
}

enum Source {
    GitHub(u64),
    Cache,
}

struct Files {
    entries: Vec<GitHubPullRequestFile>,
    next_page: Option<u32>,
}

#[derive(Default)]
pub struct Review {
    pub(super) generation: u64,
    state: State,
    pub discussion: Vec<GitHubReviewComment>,
    pub reviews: Vec<GitHubReviewComment>,
    pub threads: Vec<GitHubReviewThread>,
    pub cursors: BTreeMap<String, Option<String>>,
    pub selected_comments: HashSet<String>,
    pub check_logs: BTreeMap<String, GitHubCheckLogs>,
    pub error: Option<String>,
    pub notice: Option<String>,
}

impl Review {
    pub fn cached(&self) -> bool {
        matches!(&self.state, State::Verified(verified) if matches!(verified.source, Source::Cache))
    }

    pub(super) fn newer_than(&self, request: u64) -> bool {
        matches!(&self.state, State::Verified(verified) if matches!(verified.source, Source::GitHub(id) if id > request))
    }

    pub fn details(&self) -> Option<&GitHubPullRequestDetails> {
        match &self.state {
            State::Unloaded => None,
            State::Verified(verified) => Some(&verified.details),
        }
    }

    pub fn head(&self) -> Option<&str> {
        self.details().map(|details| details.head_ref_oid.as_str())
    }

    pub fn expected_base(&self) -> Option<&str> {
        self.details().map(|details| details.base_ref_oid.as_str())
    }

    fn file_pages(&self) -> Option<&Files> {
        match &self.state {
            State::Unloaded => None,
            State::Verified(verified) => verified.files.as_ref(),
        }
    }

    pub fn files(&self) -> &[GitHubPullRequestFile] {
        self.file_pages()
            .map_or(&[], |files| files.entries.as_slice())
    }

    pub fn files_loaded(&self) -> bool {
        self.file_pages().is_some()
    }

    pub fn next_page(&self) -> Option<u32> {
        self.file_pages().and_then(|files| files.next_page)
    }

    pub(super) fn clear_files(&mut self) {
        if let State::Verified(verified) = &mut self.state {
            verified.files = None;
        }
    }

    pub fn clear_details(&mut self) {
        self.state = State::Unloaded;
        self.generation += 1;
        self.clear_feedback();
    }

    pub fn edit_details(&mut self, edit: impl FnOnce(&mut GitHubPullRequestDetails)) -> bool {
        let State::Verified(verified) = &mut self.state else {
            return false;
        };
        let head = verified.details.head_ref_oid.clone();
        let base = verified.details.base_ref_oid.clone();
        edit(&mut verified.details);
        if verified.details.head_ref_oid != head || verified.details.base_ref_oid != base {
            verified.files = None;
            self.generation += 1;
            self.clear_feedback();
        }
        true
    }

    pub fn set_details(&mut self, details: GitHubPullRequestDetails) {
        if self.details().is_some_and(|previous| {
            previous.head_ref_oid != details.head_ref_oid
                || previous.base_ref_oid != details.base_ref_oid
        }) {
            self.clear_details();
        }
        self.accept_details(0, details);
    }

    pub(super) fn clear_feedback(&mut self) {
        self.discussion.clear();
        self.reviews.clear();
        self.threads.clear();
        self.cursors.clear();
        self.selected_comments.clear();
        self.check_logs.clear();
    }

    pub(super) fn accept_details(&mut self, request: u64, details: GitHubPullRequestDetails) {
        match &mut self.state {
            State::Verified(verified)
                if verified.details.head_ref_oid == details.head_ref_oid
                    && verified.details.base_ref_oid == details.base_ref_oid =>
            {
                verified.source = Source::GitHub(request);
                verified.details = details;
            }
            _ => {
                self.state = State::Verified(Box::new(Verified {
                    source: Source::GitHub(request),
                    details,
                    files: None,
                }))
            }
        }
    }

    pub(super) fn accept_cache(&mut self, cached: github::CachedReview, url: &str) {
        if !matches!(self.state, State::Unloaded) || cached.details.pull_request.url != url {
            return;
        }
        let files = cached
            .files
            .filter(|page| page.base_oid == cached.details.base_ref_oid)
            .map(|page| Files {
                entries: page.files,
                next_page: page.next_page,
            });
        self.state = State::Verified(Box::new(Verified {
            source: Source::Cache,
            details: cached.details,
            files,
        }));
    }

    pub fn apply_file_page(
        &mut self,
        head: &str,
        page: u32,
        files: GitHubPullRequestFiles,
    ) -> bool {
        let State::Verified(verified) = &mut self.state else {
            return false;
        };
        if head != verified.details.head_ref_oid || files.base_oid != verified.details.base_ref_oid
        {
            return false;
        }
        let target = match (&mut verified.files, page) {
            (previous, 1) => previous.insert(Files {
                entries: Vec::new(),
                next_page: None,
            }),
            (Some(previous), page) if previous.next_page == Some(page) => previous,
            _ => return false,
        };
        for file in files.files {
            if !target.entries.iter().any(|entry| entry.path == file.path) {
                target.entries.push(file);
            }
        }
        target.next_page = files.next_page.filter(|next| *next > page);
        true
    }

    pub(super) fn confirm_auto_merge(&mut self, result: caelum_protocol::GitHubAutoMergeResult) {
        if let State::Verified(verified) = &mut self.state {
            if verified.details.head_ref_oid == result.head_ref_oid {
                verified.details.state = result.state;
                verified.details.auto_merge_request = result.auto_merge_request;
            }
        }
    }

    pub fn address_prompt(&self) -> Option<String> {
        let details = self.details()?;
        let mut comments = Vec::new();
        for c in self.discussion.iter().chain(&self.reviews) {
            if self.selected_comments.contains(&c.id) {
                comments.push(format!("{}\n{}", c.url, c.body));
            }
        }
        for thread in &self.threads {
            for c in &thread.comments {
                if self.selected_comments.contains(&c.id) {
                    comments.push(format!(
                        "{}:{} ({}, {}{})\n{}\n{}\n{}",
                        thread.path,
                        thread.line.or(thread.original_line).unwrap_or(0),
                        thread.diff_side,
                        if thread.is_resolved {
                            "resolved"
                        } else {
                            "open"
                        },
                        if thread.is_outdated { ", outdated" } else { "" },
                        c.url,
                        c.diff_hunk.as_deref().unwrap_or_default(),
                        c.body
                    ));
                }
            }
        }
        if comments.is_empty() {
            return None;
        }
        Some(format!("Address these selected review comments for {} at head {}. Verify each against the current checkout before changing code.\n\n{}", details.pull_request.url, details.head_ref_oid, comments.join("\n\n---\n\n")))
    }
}
