use super::*;

impl Services {
    pub(in crate::application) fn complete(
        &mut self,
        id: u64,
        task: &Task,
        reply: Result<TaskReply, String>,
    ) -> Vec<Task> {
        let current = self.finish_slot(task, id);
        if let Task::Update(job) = task {
            self.revision += 1;
            let result = reply.and_then(|reply| match reply {
                TaskReply::Update(reply) => Ok(reply),
                _ => Err("Unexpected updater response".into()),
            });
            return self
                .updater
                .complete(job, result)
                .map(Task::Update)
                .into_iter()
                .collect();
        }
        if !current {
            return Vec::new();
        }
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) => {
                self.requests
                    .insert(task.slot(), requests::Slot::Failed(error.clone()));
                if let Some((url, _)) = task.review() {
                    self.reviews
                        .entry((task.project().into(), url.into()))
                        .or_default()
                        .error = Some(error);
                } else if !matches!(
                    task,
                    Task::Shell { .. } | Task::HostStats | Task::InstalledProviders
                ) {
                    self.checkouts
                        .entry(task.project().into())
                        .or_default()
                        .error = Some(error);
                }
                return Vec::new();
            }
        };
        let mut next = Vec::new();
        let mut merged_url = None;
        self.revision += 1;
        let checkout = self.checkouts.entry(task.project().into()).or_default();
        match (task, reply) {
            (
                Task::Shell {
                    session,
                    id,
                    stop: false,
                    ..
                },
                TaskReply::Shell(output),
            ) => {
                if checkout.shell_output.len() >= 64
                    && !checkout
                        .shell_output
                        .contains_key(&(session.clone(), id.clone()))
                {
                    checkout.shell_output.pop_first();
                }
                checkout
                    .shell_output
                    .insert((session.clone(), id.clone()), output);
            }
            (
                Task::Shell {
                    session,
                    id,
                    stop: true,
                    ..
                },
                TaskReply::Shell(_),
            ) => {
                checkout.shell_output.remove(&(session.clone(), id.clone()));
            }
            (Task::Image { project, source }, TaskReply::Image(data)) => {
                while self.images.len() >= 24
                    || self.images.values().map(String::len).sum::<usize>() + data.len()
                        > 40 * 1024 * 1024
                {
                    if self.images.pop_first().is_none() {
                        break;
                    }
                }
                self.images.insert((project.clone(), source.clone()), data);
            }
            (Task::Updates, TaskReply::Updates(update)) => self.update = Some(update),
            (Task::HostStats, TaskReply::HostStats(sample)) => match &mut self.host {
                Some(history) => history.push(sample),
                None => self.host = Some(crate::host_stats::HostHistory::from_sample(sample)),
            },
            (Task::InstalledProviders, TaskReply::InstalledProviders(providers)) => {
                self.installed_providers = Some(providers);
            }
            (Task::Usage { provider, .. }, TaskReply::Usage(usage)) => {
                checkout.usage.insert(*provider, usage);
            }
            (Task::Stats { .. }, TaskReply::Stats(stats, projects)) => {
                checkout.stats = Some(stats);
                checkout.stats_projects = projects;
            }
            (Task::Completions { query, .. }, TaskReply::Search(results)) => {
                checkout.completions = Some(results);
                checkout.completion_query = query.clone();
            }
            (Task::Directory { path, offset, .. }, TaskReply::Directory(mut page)) => {
                if *offset > 0 {
                    if let Some(previous) = checkout.directories.get(path) {
                        let mut entries = previous.entries.clone();
                        entries.append(&mut page.entries);
                        page.entries = entries;
                    }
                }
                checkout.directories.insert(path.clone(), page);
            }
            (Task::File { path, .. }, TaskReply::File(file)) => {
                checkout.files.insert(path.clone(), file);
            }
            (Task::Search { query, .. }, TaskReply::Search(results)) => {
                checkout.search = Some(results);
                checkout.search_query = query.clone();
            }
            (Task::Git { project }, TaskReply::Git(status, _)) => {
                checkout
                    .diffs
                    .retain(|path, _| status.changes.iter().any(|c| &c.path == path));
                if let Some(branch) = status
                    .branch
                    .as_ref()
                    .filter(|b| checkout.git.as_ref().and_then(|g| g.branch.as_ref()) != Some(*b))
                {
                    next.push(Task::Discover {
                        project: project.clone(),
                        branch: branch.clone(),
                    });
                }
                checkout.git = Some(status);
            }
            (Task::Diff { path, .. }, TaskReply::Diff(diff))
                if checkout
                    .git
                    .as_ref()
                    .is_none_or(|g| g.changes.iter().any(|c| &c.path == path)) =>
            {
                checkout.diffs.insert(path.clone(), diff);
            }
            (Task::CheckpointDiff { tree_oid, .. }, TaskReply::CheckpointDiff(diff)) => {
                checkout.checkpoint_diff = Some((tree_oid.clone(), diff));
            }
            (
                Task::RestoreCheckpoint {
                    project, tree_oid, ..
                },
                TaskReply::CheckpointRestored(paths),
            ) => {
                checkout.checkpoint_restored = Some((tree_oid.clone(), paths));
                checkout.checkpoint_diff = Some((
                    tree_oid.clone(),
                    crate::checkpoints::CheckpointDiff::default(),
                ));
                next.push(Task::Git {
                    project: project.clone(),
                });
            }
            (_, TaskReply::Worktrees(listing)) => {
                checkout.worktrees = listing;
                checkout.worktrees_revision = id;
            }
            (
                Task::CreateWorktree {
                    directory, project, ..
                },
                TaskReply::Worktree(_),
            ) => next.push(Task::Worktrees {
                project: project.clone(),
                directory: directory.clone(),
            }),
            (Task::RemoveWorktree { project, .. }, TaskReply::Worktree(_)) => {
                next.push(Task::Worktrees {
                    project: project.clone(),
                    directory: String::new(),
                })
            }
            (
                Task::RenameWorktree {
                    project, directory, ..
                },
                TaskReply::Worktree(_),
            ) => next.push(Task::Worktrees {
                project: project.clone(),
                directory: directory.clone(),
            }),
            (Task::Discover { project, branch }, TaskReply::Discovered(pr)) => {
                let pr = pr.filter(|pr| !self.merged_pull_requests.contains(&pr.url));
                let found = pr.as_ref().map(|pr| Discovered {
                    url: pr.url.clone(),
                    request: id,
                });
                if let Some(Some(previous)) = self
                    .discovered
                    .insert((project.clone(), branch.clone()), found)
                {
                    if pr.as_ref().is_none_or(|pr| pr.url != previous.url)
                        && !self.discovered.iter().any(|((path, _), found)| {
                            path == project && found.as_ref().is_some_and(|f| f.url == previous.url)
                        })
                    {
                        checkout.pull_requests.retain(|pr| pr.url != previous.url);
                    }
                }
                if let Some(pr) = pr {
                    if let Some(previous) =
                        checkout.pull_requests.iter_mut().find(|p| p.url == pr.url)
                    {
                        *previous = pr.clone();
                    } else {
                        checkout.pull_requests.push(pr.clone());
                    }
                    if !pr.base_ref_name.is_empty()
                        && !self
                            .discovered
                            .contains_key(&(project.clone(), pr.base_ref_name.clone()))
                    {
                        self.discovered
                            .insert((project.clone(), pr.base_ref_name.clone()), None);
                        next.push(Task::Discover {
                            project: project.clone(),
                            branch: pr.base_ref_name,
                        });
                    }
                }
            }
            (Task::ReviewCache { project, url }, TaskReply::ReviewCache(Some(cached))) => {
                let review = self
                    .reviews
                    .entry((project.clone(), url.clone()))
                    .or_default();

                review.accept_cache(*cached, url);
            }
            (Task::Details { project, url }, TaskReply::Details(details)) => {
                if details.state == "MERGED" {
                    merged_url = Some(url.clone());
                }
                if self
                    .reviews
                    .get(&(project.clone(), url.clone()))
                    .is_none_or(|review| {
                        review.head() != Some(&details.head_ref_oid)
                            || review.expected_base() != Some(&details.base_ref_oid)
                    })
                {
                    self.refresh_review(project, url);
                }
                let review = self
                    .reviews
                    .entry((project.clone(), url.clone()))
                    .or_default();
                if review.head() != Some(&details.head_ref_oid)
                    || review.expected_base() != Some(&details.base_ref_oid)
                {
                    review.replace_revision();
                }
                let head = details.head_ref_oid.clone();
                let base = details.base_ref_oid.clone();
                review.accept_details(id, *details);
                if !review.files_loaded() {
                    next.push(Task::ReviewFiles {
                        project: project.clone(),
                        url: url.clone(),
                        head: head.clone(),
                        base,
                        page: 1,
                    });
                }
                for kind in [
                    GitHubFeedbackKind::Discussion,
                    GitHubFeedbackKind::Reviews,
                    GitHubFeedbackKind::Threads,
                ] {
                    if !review.cursors.contains_key(&format!("{kind:?}")) {
                        next.push(Task::Feedback {
                            project: project.clone(),
                            url: url.clone(),
                            head: head.clone(),
                            kind,
                            cursor: None,
                            thread: None,
                        });
                    }
                }
            }
            (
                Task::ReviewFiles {
                    project,
                    url,
                    head,
                    page,
                    ..
                },
                TaskReply::ReviewFiles(files),
            ) => {
                let review = self
                    .reviews
                    .entry((project.clone(), url.clone()))
                    .or_default();
                if !review.apply_file_page(head, *page, files) {
                    review.error = Some(
                        "PR revision or page changed; refresh the review before loading more files"
                            .into(),
                    );
                }
            }
            (
                Task::Feedback {
                    project,
                    url,
                    kind,
                    thread,
                    ..
                },
                TaskReply::Feedback(page),
            ) => {
                let review = self
                    .reviews
                    .entry((project.clone(), url.clone()))
                    .or_default();
                if *kind == GitHubFeedbackKind::Replies {
                    if let Some(target) = review
                        .threads
                        .iter_mut()
                        .find(|t| Some(&t.id) == thread.as_ref())
                    {
                        append_comments(&mut target.comments, page.comments);
                        target.next_cursor = page.next_cursor;
                    }
                } else {
                    review.cursors.insert(format!("{kind:?}"), page.next_cursor);
                    match kind {
                        GitHubFeedbackKind::Discussion => {
                            append_comments(&mut review.discussion, page.comments)
                        }
                        GitHubFeedbackKind::Reviews => {
                            append_comments(&mut review.reviews, page.comments)
                        }
                        GitHubFeedbackKind::Threads => {
                            for thread in page.threads {
                                if !review.threads.iter().any(|t| t.id == thread.id) {
                                    review.threads.push(thread);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            (Task::SetAutoMerge { project, url, .. }, TaskReply::AutoMerge(result)) => {
                self.invalidate_details(project, url);
                if result.state == "MERGED" {
                    merged_url = Some(url.clone());
                }
                let review = self
                    .reviews
                    .entry((project.clone(), url.clone()))
                    .or_default();
                review.notice = Some(
                    if result.state == "MERGED" {
                        "Pull request merged."
                    } else if result.auto_merge_request.is_some() {
                        "Auto-merge enabled. GitHub will merge when requirements are met."
                    } else {
                        "Auto-merge disabled."
                    }
                    .into(),
                );
                review.confirm_auto_merge(result);
                next.push(Task::Details {
                    project: project.clone(),
                    url: url.clone(),
                });
            }
            (
                Task::Merge { project, url, .. } | Task::UpdateBranch { project, url, .. },
                TaskReply::Done,
            ) => {
                if matches!(task, Task::Merge { .. }) {
                    merged_url = Some(url.clone());
                }
                self.reviews
                    .entry((project.clone(), url.clone()))
                    .or_default()
                    .notice = Some(
                    if matches!(task, Task::Merge { .. }) {
                        "Pull request merged."
                    } else {
                        "Branch update accepted. Refresh to observe the new head."
                    }
                    .into(),
                );
                next.push(Task::Details {
                    project: project.clone(),
                    url: url.clone(),
                });
            }
            (
                Task::WriteReview {
                    project,
                    url,
                    action,
                    ..
                },
                TaskReply::ReviewWritten(result),
            ) => {
                let reload = self
                    .reviews
                    .get_mut(&(project.clone(), url.clone()))
                    .is_some_and(|review| review.confirm_write(action, result));
                if reload {
                    self.refresh_review(project, url);
                    next.push(Task::Details {
                        project: project.clone(),
                        url: url.clone(),
                    });
                }
            }
            (
                Task::CheckLogs {
                    project,
                    url,
                    head,
                    check_name,
                    ..
                },
                TaskReply::CheckLogs(logs),
            ) => {
                if let Some(review) = self.reviews.get_mut(&(project.clone(), url.clone())) {
                    if review.head() == Some(head.as_str()) {
                        review.check_logs.insert(check_name.clone(), logs);
                    }
                }
            }
            _ => {}
        }
        if let Some(url) = merged_url {
            self.merged_pull_requests.insert(url.clone());
            for checkout in self.checkouts.values_mut() {
                checkout.pull_requests.retain(|pr| pr.url != url);
            }
        }
        next
    }
}

fn append_comments(target: &mut Vec<GitHubReviewComment>, comments: Vec<GitHubReviewComment>) {
    for comment in comments {
        if !target.iter().any(|c| c.id == comment.id) {
            target.push(comment);
        }
    }
}
