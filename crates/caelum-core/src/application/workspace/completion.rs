use super::{Pending, Session, Workspace};
use crate::application::{Attachment, Command, Provider, Reply, Scope, Task, TaskReply};

impl Workspace {
    pub(super) fn complete(
        &mut self,
        request: u64,
        pending: Pending,
        result: Result<Reply, String>,
    ) {
        if let Pending::CheckHandoff(i) = pending {
            self.complete_handoff_check(i, request, result);
            return;
        }
        if let Pending::Remote(lane) = pending {
            self.complete_remote(request, lane, result);
            return;
        }
        if let Pending::ConversationControl(i, owner, operation) = pending {
            self.finish_control(i, owner, operation, result);
            return;
        }
        if let Pending::Fork(pending) = pending {
            self.complete_fork(request, pending, result);
            return;
        }
        if let Pending::Home(pending) = pending {
            self.complete_home(request, pending, result);
            return;
        }
        if let Pending::VerifyOpenCodeMove(index) = pending {
            self.finish_move_check(request, index, result);
            return;
        }
        if let Pending::RegenerateTitle(index) = pending {
            self.finish_title_update(index, request);
            return;
        }
        if let Pending::Attachment(index, name) = pending {
            self.sessions[index].attaching = self.sessions[index].attaching.saturating_sub(1);
            let attachment = match result {
                Ok(Reply::Task(TaskReply::File(file))) => {
                    Attachment::new(file.path, file.content, None, file.truncated)
                }
                Ok(Reply::Task(TaskReply::Image(data))) => {
                    let mut attachment = Attachment::new(name, String::new(), None, false);
                    attachment.kind = crate::application::AttachmentKind::Image;
                    attachment.content = data;
                    attachment
                }
                result => {
                    self.sessions[index].attachment_error(
                        name,
                        result
                            .err()
                            .unwrap_or_else(|| "Unexpected attachment response".into()),
                    );
                    return;
                }
            };
            self.attach(index, attachment);
            return;
        }
        if let Pending::Mention {
            index,
            side,
            name,
            generation,
            draft,
            token,
            replacement,
        } = pending
        {
            let session = &mut self.sessions[index];
            session.attaching = session.attaching.saturating_sub(1);
            if self.selected != Some(index)
                || session.archived()
                || generation != session.completion_generation
                || draft
                    != (if side {
                        &session.side_draft
                    } else {
                        &session.draft
                    })
                    .as_str()
            {
                return;
            }
            let attachment = match result {
                Ok(Reply::Task(TaskReply::File(file))) => {
                    Attachment::new(file.path, file.content, None, file.truncated)
                }
                Ok(Reply::Task(TaskReply::Details(details))) => Attachment::pull_request(&details),
                result => {
                    let error = result
                        .err()
                        .unwrap_or_else(|| "Unexpected context response".into());
                    if side {
                        session.side_error = Some(error);
                    } else {
                        session.attachment_error(name, error);
                    }
                    return;
                }
            };
            let attached = if side {
                let attachments = &mut self.sessions[index].side_attachments;
                attachments.retain(|a| !(a.kind == attachment.kind && a.path == attachment.path));
                if attachments.len() < 16 {
                    attachments.push(attachment);
                    true
                } else {
                    self.sessions[index].side_error = Some("Attach at most 16 items".into());
                    false
                }
            } else {
                self.attach(index, attachment)
            };
            if attached {
                let (text, caret) = token.replace(&draft, &replacement);
                if side {
                    self.sessions[index].side_draft = text;
                    self.sessions[index].side_completion_caret = Some(caret);
                } else {
                    self.sessions[index].draft = text;
                    self.sessions[index].completion_caret = Some(caret);
                }
            }
            return;
        }
        if let Pending::Task(task) = pending {
            self.complete_task(request, task, result);
            return;
        }
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => {
                match pending {
                    Pending::Inbox(index, _) => self.sessions[index].error = Some(error),
                    Pending::Enqueue(index, id) | Pending::RemoveQueued(index, id) => {
                        self.sessions[index]
                            .inbox
                            .complete_mutation(&id, request, Err(error));
                    }
                    Pending::Control(index) => {
                        self.complete_native_command(index, request, Err(error))
                    }
                    Pending::Generate(index, _, generation) => {
                        if self.sessions[index].side_generation == generation {
                            self.sessions[index].generating = false;
                            self.sessions[index].side_error = Some(error);
                        }
                    }
                    Pending::Notify => {}
                    Pending::Load => {
                        self.store_loaded(request, Err(error));
                    }
                    Pending::Save => self.store_saved(request, Err(error)),
                    Pending::Remote(_)
                    | Pending::CheckHandoff(..)
                    | Pending::Home(_)
                    | Pending::ConversationControl(..)
                    | Pending::Fork(_)
                    | Pending::VerifyOpenCodeMove(_)
                    | Pending::Task(_)
                    | Pending::Mention { .. }
                    | Pending::Attachment(..)
                    | Pending::RegenerateTitle(..) => {
                        unreachable!()
                    }
                    Pending::Commands(scope) => {
                        self.notices.insert(scope, error);
                    }
                    Pending::Shells(index, _) => self.sessions[index].error = Some(error),
                    Pending::Archive(index) => self.complete_archive(index, request, Err(error)),
                    Pending::DeleteSession(index) => {
                        self.complete_delete(index, request, Err(error))
                    }
                    Pending::Details(_) => {}
                    Pending::Open(_, _) => {
                        if request == self.latest_open {
                            self.opening = false;
                            self.error = Some(error);
                        }
                    }
                    Pending::List(scope)
                    | Pending::ListArchived(scope)
                    | Pending::Choices(scope) => {
                        self.notices.insert(scope, error);
                    }
                    Pending::Resume(index) | Pending::History(index) => {
                        self.fail_history(index, request, error);
                    }
                    Pending::Start(index) => self.complete_creation(index, request, Err(error)),
                    Pending::Send(index) => self.complete_admission(index, request, Err(error)),
                    Pending::Cancel(index) => self.complete_stop(index, request, Err(error)),
                    Pending::Request {
                        provider,
                        project,
                        session,
                    } => {
                        for target in self.sessions.iter_mut().filter(|target| {
                            target.scope.project == project
                                && target.scope.provider.as_str() == provider
                                && session
                                    .as_ref()
                                    .is_none_or(|id| target.remote_id.as_ref() == Some(id))
                        }) {
                            target.error = Some(error.clone());
                        }
                        if let Some(provider) =
                            Provider::ALL.into_iter().find(|p| p.as_str() == provider)
                        {
                            self.notices.insert(Scope { project, provider }, error);
                        }
                    }
                }
                return;
            }
        };
        match (pending, reply) {
            (Pending::Inbox(index, revision), Reply::Inbox(items)) => {
                self.sessions[index].inbox.snapshot(revision, &items)
            }
            (Pending::Enqueue(index, id) | Pending::RemoveQueued(index, id), reply) => {
                self.sessions[index]
                    .inbox
                    .complete_mutation(&id, request, done(reply));
            }
            (Pending::Control(index), reply) => {
                self.complete_native_command(index, request, done(reply));
            }
            (Pending::Generate(index, prompt, generation), Reply::Generated(text)) => {
                if self.sessions[index].side_generation == generation {
                    self.sessions[index].generating = false;
                    self.sessions[index].side_question = Some((prompt, text));
                }
            }
            (Pending::Notify, Reply::Done) => {}
            (Pending::Details(index), Reply::Details(details)) => {
                let project = self.sessions[index].scope.project.clone();
                for branch in &details.branches {
                    self.task(Task::Discover {
                        project: project.clone(),
                        branch: branch.clone(),
                    });
                }
                self.sessions[index].details = Some(details);
            }
            (Pending::Load, Reply::Snapshot(snapshot)) => {
                if !self.store_loaded(request, Ok(snapshot.as_deref())) {
                    return;
                }
                if let Some(snapshot) = snapshot {
                    let snapshot = *snapshot;
                    self.home.restore(snapshot.home);
                    self.remote.saved = snapshot.remote;
                    self.managed_worktrees = snapshot.managed_worktrees;
                    self.preferences = snapshot.preferences;
                    for (project, saved) in &self.preferences.files {
                        let c = self.services.checkouts.entry(project.clone()).or_default();
                        c.tabs = saved
                            .tabs
                            .iter()
                            .filter(|p| {
                                !p.is_empty()
                                    && !std::path::Path::new(p).is_absolute()
                                    && !std::path::Path::new(p)
                                        .components()
                                        .any(|c| matches!(c, std::path::Component::ParentDir))
                            })
                            .take(40)
                            .cloned()
                            .collect();
                        c.selected_file = saved
                            .selected
                            .clone()
                            .filter(|p| c.tabs.contains(p))
                            .or_else(|| c.tabs.first().cloned());
                    }
                    self.project_roots = snapshot.project_roots;
                    if self.sessions.is_empty() {
                        self.projects = snapshot.projects;
                        self.sessions = snapshot
                            .sessions
                            .into_iter()
                            .map(|saved| {
                                let mut session = Session::new(saved.scope);
                                session.native_handoff = saved.native_handoff;
                                if let Some(h) = &session.native_handoff {
                                    session.conversation.handoff(&h.chat);
                                }
                                session.vm_link = saved.vm_link;
                                if saved.remote_id.is_some() {
                                    session.invalidate_history();
                                }
                                session.remote_id = saved.remote_id;
                                session.title = saved.title;
                                session.last_activity = saved.last_activity;
                                session.title_state = super::titles::TitleState::restored(
                                    saved.manual_title,
                                    saved.title_refresh_turns,
                                );
                                session.options = saved.options;
                                session.options_dirty = true;
                                session.draft = saved.draft;
                                session.attachments = saved.attachments;
                                session.failed_prompt = saved.failed_prompt;
                                session.queue = saved.queue;
                                session.inbox.items = saved
                                    .inbox
                                    .into_iter()
                                    .map(super::super::SavedInboxPrompt::restore)
                                    .collect();
                                if let Some(pending) = saved.pending_submission {
                                    session.queue.insert(0, pending);
                                }
                                session.queue_paused = true;
                                session.restore_archive(saved.archived, 0);
                                session.reading = saved.reading;
                                session.unread = saved.unread;
                                session
                            })
                            .collect();
                        for (i, s) in self
                            .sessions
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| !s.archived())
                        {
                            self.remembered.insert(s.scope.project.clone(), i);
                            self.remembered_checkouts.insert(s.scope.project.clone(), i);
                        }
                        self.remembered
                            .extend(snapshot.remembered.into_iter().filter(|(_, i)| {
                                *i < self.sessions.len() && !self.sessions[*i].archived()
                            }));
                        self.remembered_checkouts.extend(
                            self.remembered
                                .iter()
                                .filter(|(path, i)| self.sessions[**i].scope.project == **path)
                                .map(|(path, i)| (path.clone(), *i)),
                        );
                        self.remembered_checkouts.extend(
                            snapshot
                                .remembered_checkouts
                                .into_iter()
                                .filter(|(path, i)| {
                                    self.sessions
                                        .get(*i)
                                        .is_some_and(|s| s.scope.project == *path && !s.archived())
                                }),
                        );
                        let details: Vec<_> = self
                            .sessions
                            .iter()
                            .enumerate()
                            .filter(|(_, s)| self.project_is_open(&s.scope.project))
                            .filter(|(_, s)| {
                                crate::remote::native::checkout_id(&s.scope.project).is_none()
                            })
                            .filter_map(|(i, s)| s.remote_id.as_ref().map(|_| i))
                            .collect();
                        self.detail_queue.extend(details);
                        if let Some(index) = snapshot.selected.filter(|i| *i < self.sessions.len())
                        {
                            let navigation = self.navigation;
                            self.select(index);

                            self.navigation = navigation;
                            self.refresh_project();
                        }
                    }
                }
            }
            (Pending::Save, Reply::Done) => self.store_saved(request, Ok(())),
            (Pending::Commands(scope), Reply::Commands(commands)) => {
                self.provider_commands.insert(scope, commands);
            }
            (Pending::Shells(index, revision), Reply::Shells(shells)) => {
                let session = &mut self.sessions[index];
                if revision != session.shell_revision {
                    if let Some(key) = session.key() {
                        let revision = session.shell_revision;
                        self.request(Command::Shells(key), Pending::Shells(index, revision));
                    }
                    return;
                }
                session.shells =
                    shells
                        .into_iter()
                        .filter(|shell| {
                            !session.conversation.entries.iter().any(|e| {
                                e.id == format!("opencode-shell-{}", shell.id) && e.complete()
                            })
                        })
                        .collect();
                if let (Some(remote), Some(checkout)) = (
                    &session.remote_id,
                    self.services.checkouts.get_mut(&session.scope.project),
                ) {
                    checkout.shell_output.retain(|(owner, id), _| {
                        owner != remote || session.shells.iter().any(|shell| &shell.id == id)
                    });
                }
            }
            (Pending::Archive(index), reply) => self.complete_archive(index, request, done(reply)),
            (Pending::DeleteSession(index), reply) => {
                self.complete_delete(index, request, done(reply))
            }
            (Pending::Open(provider, navigation), Reply::Project(project)) => {
                if self.closed_since(&project.path, request) {
                    if request == self.latest_open {
                        self.opening = false;
                    }
                    return;
                }
                if !self.projects.iter().any(|p| p.path == project.path) {
                    self.projects.push(project.clone());
                }

                let details: Vec<_> = self
                    .sessions
                    .iter()
                    .enumerate()
                    .filter(|(i, s)| {
                        self.group_path(&s.scope.project) == self.group_path(&project.path)
                            && s.remote_id.is_some()
                            && s.details.is_none()
                            && !self.detail_queue.contains(i)
                            && !self
                                .pending
                                .values()
                                .any(|p| matches!(p, Pending::Details(index) if index == i))
                    })
                    .map(|(i, _)| i)
                    .collect();
                self.detail_queue.extend(details);
                if request == self.latest_open {
                    self.opening = false;
                    if navigation == self.navigation {
                        self.select_project(&project.path);
                        if let Some(provider) = provider {
                            self.project_draft(Scope {
                                project: project.path.clone(),
                                provider,
                            });
                        }
                        self.refresh_project();
                    }
                }
            }
            (Pending::List(scope), Reply::Sessions(summaries)) => {
                if self.creating(&scope) {
                    self.deferred_lists.insert(scope, (request, summaries));
                } else {
                    self.apply_summaries(request, &scope, summaries, false);
                }
            }
            (Pending::ListArchived(scope), Reply::Sessions(summaries)) => {
                self.apply_summaries(request, &scope, summaries, true);
            }
            (Pending::Choices(scope), Reply::Choices(choices)) => {
                for session in self
                    .sessions
                    .iter_mut()
                    .filter(|s| s.scope == scope && s.remote_id.is_none() && !s.submitting())
                {
                    if session.options.model.is_none() {
                        session.options.model = choices.default_model.clone();
                    }
                }
                self.choices.insert(scope, choices);
            }
            (Pending::Resume(index), Reply::Session(summary)) => {
                self.complete_resume(request, index, summary, None);
            }
            (Pending::Resume(index), Reply::Resumed(summary, page)) => {
                self.complete_resume(request, index, summary, Some(page));
            }
            (Pending::Resume(index), Reply::SessionMoved(change)) => {
                self.resume_moved(request, index, change);
            }
            (Pending::History(index), Reply::History(page)) => {
                self.complete_history(index, request, page);
            }
            (Pending::Resume(index) | Pending::History(index), _) => {
                self.fail_history(index, request, "Unexpected history response".into());
            }
            (Pending::Start(index), reply) => {
                let result = match reply {
                    Reply::Session(summary) => Ok(summary),
                    _ => Err("Unexpected session creation response".into()),
                };
                self.complete_creation(index, request, result);
            }
            (Pending::Send(index), reply) => self.complete_admission(index, request, done(reply)),
            (Pending::Cancel(index), reply) => self.complete_stop(index, request, done(reply)),
            (Pending::Request { .. }, Reply::Done) => {}
            _ => self.error = Some("Unexpected application response".into()),
        }
    }
}

fn done(reply: Reply) -> Result<(), String> {
    match reply {
        Reply::Done => Ok(()),
        _ => Err("Unexpected application response".into()),
    }
}
