use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crate::{
    antigravity::AntigravityProvider, approvals::ApprovalBroker, codex::CodexProvider,
    opencode::OpenCodeProvider, processes::ProcessRegistry, projects::Project,
    questions::QuestionBroker,
};

use super::{Options, Provider, SessionKey, Summary, Task};

mod antigravity;
#[path = "checkout_watch.rs"]
mod checkout_watch;
mod conversation;
mod handoff;
mod messages;
mod native;
mod opencode;
mod worker;

pub use messages::Command;
pub use messages::Event;
pub use messages::OpenCodeMove;
pub use messages::Reply;

/// Shared by desktop adapters; constructing this does not start a provider.
#[derive(Clone)]
pub struct ProviderRuntime {
    side_output: crate::side_questions::Output,
    pub(super) usage: crate::telemetry::Store,
    vm: Option<Arc<crate::remote::native::Access>>,
    pub codex: CodexProvider,
    pub opencode: OpenCodeProvider,
    pub antigravity: AntigravityProvider,
    pub approvals: ApprovalBroker,
    pub questions: QuestionBroker,
    pub processes: Arc<ProcessRegistry>,
    remote: Arc<crate::remote::Backend>,
    pub worktrees: crate::worktrees::WorktreeClient,
    pub branches: crate::session_branches::SessionBranches,
    pub(super) review_cache: Arc<Mutex<Option<crate::github::cache::Cache>>>,
    pub(super) updates: crate::updates::Backend,
    pub(super) runner: crate::runner::Backend,
}

impl Default for ProviderRuntime {
    fn default() -> Self {
        let processes = Arc::new(ProcessRegistry::default());
        let approvals = ApprovalBroker::default();
        let questions = QuestionBroker::default();
        let (p, a, q) = (processes.clone(), approvals.clone(), questions.clone());
        Self {
            side_output: Default::default(),
            usage: Default::default(),
            vm: None,
            codex: CodexProvider::new(p.clone(), a.clone(), q.clone()),
            opencode: OpenCodeProvider::new(p.clone(), a.clone(), q.clone()),
            antigravity: AntigravityProvider::new(p.clone(), a, q),
            runner: crate::runner::Backend::new(p),
            processes,
            remote: Arc::default(),
            approvals,
            questions,
            worktrees: crate::worktrees::WorktreeClient::new(
                std::env::var_os("HOME").unwrap_or_default().into(),
            ),
            branches: crate::session_branches::SessionBranches::new(None),
            review_cache: Arc::default(),
            updates: Default::default(),
        }
    }
}

impl ProviderRuntime {
    /// Persist Git attribution and review metadata beside the workspace that owns the reads.
    pub(crate) fn open_metadata(&self, path: &std::path::Path) -> Result<(), String> {
        *self.review_cache.lock().unwrap_or_else(|e| e.into_inner()) =
            Some(crate::github::cache::Cache::for_store(path));
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            let db = crate::database::WorkspaceDatabase::open(parent.join("branches.sqlite3"))
                .map_err(|e| e.to_string())?;
            self.branches.reset(db)?;
            self.usage.open(path);
            self.antigravity
                .open_metadata(parent.join("antigravity-sessions.json"))?;
        }
        Ok(())
    }

    pub fn installed_providers(&self) -> Vec<Provider> {
        Provider::ALL
            .into_iter()
            .filter(|provider| match provider {
                Provider::Codex => self.codex.is_installed(),
                Provider::OpenCode => self.opencode.is_installed(),
                Provider::Antigravity => self.antigravity.is_installed(),
            })
            .collect()
    }

    pub fn shutdown(&self) {
        if let Some(vm) = &self.vm {
            vm.live.store(false, Ordering::Release);
        }
        self.remote.shutdown();
        self.updates.shutdown();
        self.runner.shutdown();
        self.approvals.shutdown();

        self.processes.shutdown();
        self.codex.shutdown();
        self.opencode.shutdown();
        self.antigravity.shutdown();
    }

    fn execute(&self, command: &Command) -> Result<Reply, String> {
        if let Some(vm) = &self.vm {
            if let Some(result) = vm.checkout_command(command, &self.branches) {
                return result;
            }
        }
        let result = match command {
            Command::RunCommand {
                project,
                run_id,
                command,
            } => self.runner.start(project, *run_id, command),
            Command::StopCommand { project, run_id } => {
                self.runner.stop(project, *run_id);
                Ok(())
            }
            Command::RecordUsage(_) => return Err("Turn usage requires its local writer.".into()),
            Command::SideTurn(key, turn) => {
                return crate::side_questions::run(self, key, turn, &self.side_output)
                    .map(Reply::Generated);
            }
            Command::Remote(op) => return self.remote.execute(op).map(Reply::Remote),
            Command::Inbox(key) => {
                return self
                    .opencode
                    .inbox(&key.scope.project, &key.id)
                    .map(Reply::Inbox)
                    .map_err(|e| e.to_string())
            }
            Command::Enqueue(key, id, text) => self
                .opencode
                .enqueue_prompt(&key.scope.project, &key.id, id, text)
                .map_err(|e| e.to_string()),
            Command::RemoveQueued(key, id) => self
                .opencode
                .cancel_queued_prompt(&key.scope.project, &key.id, id)
                .map_err(|e| e.to_string()),
            Command::Generate(key, prompt) => {
                return self
                    .opencode
                    .generate_from_context(&key.scope.project, &key.id, prompt)
                    .map(Reply::Generated)
                    .map_err(|e| e.to_string())
            }
            Command::RegenerateTitle(key) => match key.scope.provider {
                Provider::OpenCode => self
                    .opencode
                    .regenerate_title(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => self
                    .antigravity
                    .regenerate_title(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::Codex => {
                    Err("Codex does not support regenerating session titles.".into())
                }
            },
            Command::Rename(key, title) => match key.scope.provider {
                Provider::Codex => self
                    .codex
                    .rename(&key.scope.project, &key.id, title)
                    .map_err(|e| e.to_string()),
                Provider::OpenCode => self
                    .opencode
                    .set_title(&key.scope.project, &key.id, title)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => self
                    .antigravity
                    .rename(&key.scope.project, &key.id, title)
                    .map_err(|e| e.to_string()),
            },
            Command::Export(key, title, format, path) => self.export(key, title, *format, path),
            Command::Rewind {
                key,
                before,
                tip,
                files,
            } => match key.scope.provider {
                Provider::Codex if !files => self
                    .codex
                    .rewind(&key.scope.project, &key.id, before, tip)
                    .map_err(|e| e.to_string()),
                Provider::Codex => Err("Codex rewinds conversation history only.".into()),
                Provider::OpenCode => self
                    .opencode
                    .rewind(&key.scope.project, &key.id, before, tip, *files)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => {
                    Err("Antigravity ACP does not support rewinding sessions.".into())
                }
            },
            Command::CheckpointFiles(key, from, to) => {
                if key.scope.provider != Provider::OpenCode {
                    return Err("Codex does not restore checkout files.".into());
                }
                return self
                    .opencode
                    .checkpoint_files(&key.scope.project, &key.id, from, to)
                    .map(Reply::CheckpointFiles)
                    .map_err(|e| e.to_string());
            }
            Command::Sound(volume) => {
                #[cfg(target_os = "macos")]
                if *volume > 0 {
                    std::process::Command::new("/usr/bin/afplay")
                        .args([
                            "-v",
                            &(f32::from(*volume).min(100.0) / 100.0).to_string(),
                            "/System/Library/Sounds/Glass.aiff",
                        ])
                        .status()
                        .map_err(|e| e.to_string())?;
                }
                #[cfg(not(target_os = "macos"))]
                let _ = volume;
                Ok(())
            }
            Command::Notify(_) => Err("Desktop notifications require a desktop handler".into()),
            Command::Task(task) => return task.execute(self).map(Reply::Task),
            Command::Load(store) => {
                store.migrate_default()?;
                let snapshot = store.load_workspace()?;
                self.open_metadata(&store.0)?;
                return Ok(Reply::Snapshot(snapshot.map(Box::new)));
            }
            Command::Save(store, state) => return store.save(state).map(|()| Reply::Done),
            Command::Commands(scope) => {
                return match scope.provider {
                    Provider::OpenCode => self
                        .opencode
                        .commands(&scope.project)
                        .map(Reply::Commands)
                        .map_err(|e| e.to_string()),
                    Provider::Antigravity => self
                        .antigravity
                        .commands(&scope.project)
                        .map(Reply::Commands)
                        .map_err(|e| e.to_string()),
                    Provider::Codex => Ok(Reply::Commands(Vec::new())),
                };
            }
            Command::Shells(key) => {
                return self
                    .opencode
                    .background_shells(&key.scope.project, &key.id)
                    .map(Reply::Shells)
                    .map_err(|e| e.to_string())
            }
            Command::Archive(key, archived) => match key.scope.provider {
                Provider::Codex => self
                    .codex
                    .set_session_archived(&key.scope.project, &key.id, *archived)
                    .map_err(|e| e.to_string()),
                Provider::OpenCode => self
                    .opencode
                    .set_session_archived(&key.scope.project, &key.id, *archived)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => self
                    .antigravity
                    .set_session_archived(&key.scope.project, &key.id, *archived)
                    .map_err(|e| e.to_string()),
            },
            Command::DeleteSession(key) => match key.scope.provider {
                Provider::Codex => self
                    .codex
                    .delete_session(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::OpenCode => self
                    .opencode
                    .delete_session(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => self
                    .antigravity
                    .delete_session(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
            },
            Command::Details(key) => {
                let details = match key.scope.provider {
                    Provider::Codex => self
                        .codex
                        .session_details(&key.scope.project, &key.id)
                        .map_err(|e| e.to_string())?,
                    Provider::OpenCode => self
                        .opencode
                        .session_details(&key.scope.project, &key.id)
                        .map_err(|e| e.to_string())?,
                    Provider::Antigravity => self
                        .antigravity
                        .session_details(&key.scope.project, &key.id)
                        .map_err(|e| e.to_string())?,
                };
                return Ok(Reply::Details(self.branches.details(
                    &key.scope.project,
                    key.scope.provider.as_str(),
                    &key.id,
                    details,
                )));
            }
            Command::Open(path) => {
                return Project::open(path)
                    .map(|project| Reply::Project(project.info()))
                    .map_err(|e| e.to_string());
            }
            Command::List(scope) => {
                let summaries = match scope.provider {
                    Provider::Codex => self
                        .codex
                        .list_sessions(&scope.project)
                        .map_err(|e| e.to_string())?
                        .threads
                        .into_iter()
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.id,
                            },
                            title: s.name.filter(|v| !v.is_empty()).unwrap_or(s.preview),
                            updated_at: s.updated_at.map(|at| at.saturating_mul(1000)),
                            working: s.is_working,
                            options: Options::default(),
                        })
                        .collect(),
                    Provider::OpenCode => self
                        .opencode
                        .list_sessions(&scope.project)
                        .map_err(|e| e.to_string())?
                        .sessions
                        .into_iter()
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.id,
                            },
                            title: s.title.unwrap_or_default(),
                            updated_at: s.updated_at,
                            working: s.is_working,
                            options: Options {
                                model: s.model,
                                effort: s.effort,
                                agent: s.agent,
                            },
                        })
                        .collect(),
                    Provider::Antigravity => self.list_antigravity(scope)?,
                };
                return Ok(Reply::Sessions(summaries));
            }
            Command::ListArchived(scope) => {
                let summaries = match scope.provider {
                    Provider::Codex => self
                        .codex
                        .list_archived_sessions(&scope.project)
                        .map_err(|e| e.to_string())?
                        .threads
                        .into_iter()
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.id,
                            },
                            title: s.name.filter(|v| !v.is_empty()).unwrap_or(s.preview),
                            updated_at: s.updated_at.map(|at| at.saturating_mul(1000)),
                            working: s.is_working,
                            options: Options::default(),
                        })
                        .collect(),
                    Provider::OpenCode => self
                        .opencode
                        .list_archived_sessions(&scope.project)
                        .map_err(|e| e.to_string())?
                        .sessions
                        .into_iter()
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.id,
                            },
                            title: s.title.unwrap_or_default(),
                            updated_at: s.updated_at,
                            working: s.is_working,
                            options: Options {
                                model: s.model,
                                effort: s.effort,
                                agent: s.agent,
                            },
                        })
                        .collect(),
                    Provider::Antigravity => self.list_archived_antigravity(scope)?,
                };
                return Ok(Reply::Sessions(summaries));
            }
            Command::Choices(scope) => {
                return match scope.provider {
                    Provider::Codex => self
                        .codex
                        .choices(&scope.project)
                        .map_err(|e| e.to_string()),
                    Provider::OpenCode => self
                        .opencode
                        .choices(&scope.project)
                        .map_err(|e| e.to_string()),
                    Provider::Antigravity => self
                        .antigravity
                        .choices(&scope.project)
                        .map_err(|e| e.to_string()),
                }
                .map(Reply::Choices);
            }
            Command::Fork(key, boundary) => return self.fork(key, boundary),
            Command::EditBranch(key, boundary, text, attachments) => {
                return self.edit_branch(key, boundary, text, attachments);
            }
            Command::Start(scope, options) => {
                return match scope.provider {
                    Provider::Codex => self
                        .codex
                        .start_session_with_model(&scope.project, options.model.as_deref())
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.thread.id,
                            },
                            title: s.thread.name.unwrap_or(s.thread.preview),
                            updated_at: s.thread.updated_at.map(|at| at.saturating_mul(1000)),
                            working: s.thread.is_working,
                            options: Options {
                                model: s.model,
                                effort: s.effort,
                                agent: None,
                            },
                        })
                        .map_err(|e| e.to_string()),
                    Provider::OpenCode => self
                        .opencode
                        .start_session_with_options(
                            &scope.project,
                            options.model.as_deref(),
                            options.effort.as_deref(),
                            options.agent.as_deref(),
                        )
                        .map(|s| Summary {
                            key: SessionKey {
                                scope: scope.clone(),
                                id: s.session.id,
                            },
                            title: s.session.title.unwrap_or_default(),
                            updated_at: s.session.updated_at,
                            working: s.session.is_working,
                            options: Options {
                                model: s.session.model,
                                effort: s.session.effort,
                                agent: s.session.agent,
                            },
                        })
                        .map_err(|e| e.to_string()),
                    Provider::Antigravity => self.start_antigravity(scope, options),
                }
                .map(Reply::Session);
            }
            Command::VerifyOpenCodeMove { from, destination } => {
                return self.verify_opencode_move(from, destination)
            }
            Command::Resume(key) => {
                return match key.scope.provider {
                    Provider::Codex => self
                        .codex
                        .resume_session(&key.scope.project, &key.id)
                        .map(|s| Summary {
                            key: key.clone(),
                            title: s.thread.name.unwrap_or(s.thread.preview),
                            updated_at: s.thread.updated_at.map(|at| at.saturating_mul(1000)),
                            working: s.thread.is_working,
                            options: Options {
                                model: s.model,
                                effort: s.effort,
                                agent: None,
                            },
                        })
                        .map(Reply::Session)
                        .map_err(|e| e.to_string()),
                    Provider::OpenCode => self.resume_opencode(key),
                    Provider::Antigravity => self.resume_antigravity(key),
                };
            }
            Command::History(key, cursor) => {
                return self
                    .history_page(key, cursor.as_deref())
                    .map(Reply::History);
            }
            Command::HandoffChat(key) => {
                return self.handoff_chat(key).map(Reply::HandoffChat);
            }
            Command::CodexControlHistory(key, action) => {
                if key.scope.provider != Provider::Codex {
                    return Err("Expected a Codex session".into());
                }
                self.codex
                    .check_history_action(&key.scope.project, &key.id, *action)
                    .map_err(|e| e.to_string())?;
                return self.history_page(key, None).map(Reply::History);
            }
            Command::RunOpenCodeCommand(key, prompt, options) => {
                self.run_opencode_command(key, prompt, options)
            }
            Command::Send(key, prompt, options) => match key.scope.provider {
                Provider::Codex => self
                    .codex
                    .send_prompt_with_options(
                        &key.scope.project,
                        &key.id,
                        prompt,
                        options.model.as_deref(),
                        options.effort.as_deref(),
                    )
                    .map_err(|e| e.to_string()),
                Provider::OpenCode => {
                    if let Some(agent) = &options.agent {
                        self.opencode
                            .select_agent(&key.scope.project, &key.id, agent)
                            .map_err(|e| e.to_string())?;
                    }
                    self.opencode
                        .send_prompt_with_options(
                            &key.scope.project,
                            &key.id,
                            prompt,
                            None,
                            options.model.as_deref(),
                            options.effort.as_deref(),
                        )
                        .map_err(|e| e.to_string())
                }
                Provider::Antigravity => self
                    .antigravity
                    .send_prompt_with_options(
                        &key.scope.project,
                        &key.id,
                        prompt,
                        options.model.as_deref(),
                        options.effort.as_deref(),
                        options.agent.as_deref(),
                    )
                    .map_err(|e| e.to_string()),
            },
            Command::Cancel(key) => match key.scope.provider {
                Provider::Codex => self
                    .codex
                    .cancel_turn(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::OpenCode => self
                    .opencode
                    .cancel_turn(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
                Provider::Antigravity => self
                    .antigravity
                    .cancel_turn(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string()),
            },
            Command::Approve(prompt, decision) => self
                .approvals
                .resolve_prompt(prompt, *decision)
                .map_err(|e| e.to_string()),
            Command::Answer(prompt, answer) => self.questions.resolve(
                &prompt.provider,
                &prompt.project_path,
                &prompt.session_id,
                &prompt.request_id,
                answer.clone(),
            ),
        };
        result.map(|()| Reply::Done)
    }
}

type Execute = Arc<dyn Fn(&Command) -> Result<Reply, String> + Send + Sync>;

/// Background I/O with a bounded UI inbox. The UI drains a batch once per frame;
/// event arrival wakes it, so idle windows need no polling timer.
pub struct Runtime {
    usage: crate::telemetry::Store,
    checkout_watch: Option<mpsc::Sender<Vec<String>>>,
    watched_projects: Vec<String>,
    providers: Option<ProviderRuntime>,
    mailboxes: worker::Mailboxes,
    inbox: Option<Receiver<Event>>,
    sender: SyncSender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
    closed: Arc<AtomicBool>,
    workers: Option<worker::Workers>,
    forwarders: Vec<JoinHandle<()>>,
}

impl Runtime {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::with_providers(ProviderRuntime::default(), wake)
    }

    pub fn with_providers(
        providers: ProviderRuntime,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let backend = providers.clone();
        let mut runtime = Self::with_executor(
            Arc::new(move |command| backend.execute(command)),
            Arc::new(wake),
        );
        let local = providers.vm.is_none();
        runtime.usage = providers.usage.clone();
        providers
            .side_output
            .connect(runtime.sender.clone(), runtime.wake.clone());
        if local {
            providers
                .updates
                .observe(runtime.sender.clone(), runtime.wake.clone());
            providers
                .runner
                .observe(runtime.sender.clone(), runtime.wake.clone());
        }
        let branches = providers.branches.clone();
        runtime.forward(providers.codex.subscribe(), move |event| {
            let update = branches.codex_event(&event);
            std::iter::once(Event::Codex(event))
                .chain(update.map(Event::Branch))
                .collect()
        });
        if local {
            let branches = providers.branches.clone();
            runtime.forward(providers.opencode.subscribe(), move |event| {
                let update = branches.opencode_event(&event);
                std::iter::once(Event::OpenCode(event))
                    .chain(update.map(Event::Branch))
                    .collect()
            });
            let branches = providers.branches.clone();
            runtime.forward(providers.antigravity.subscribe(), move |event| {
                let update = branches.antigravity_event(&event);
                std::iter::once(Event::Antigravity(event))
                    .chain(update.map(Event::Branch))
                    .collect()
            });
        }
        runtime.forward(providers.approvals.subscribe(), |value| {
            vec![Event::Approvals(value)]
        });
        runtime.forward(providers.questions.subscribe(), |value| {
            vec![Event::Questions(value)]
        });
        runtime.providers = Some(providers);
        if local {
            runtime.start_checkout_watch();
        }
        runtime
    }

    pub(super) fn with_executor(execute: Execute, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (sender, inbox) = mpsc::sync_channel(1024);
        let closed = Arc::new(AtomicBool::new(false));
        let (mailboxes, workers) =
            worker::Workers::start(execute, sender.clone(), wake.clone(), closed.clone());
        Self {
            checkout_watch: None,
            usage: Default::default(),
            watched_projects: Vec::new(),
            providers: None,
            mailboxes,
            inbox: Some(inbox),
            sender,
            wake,
            closed,
            workers: Some(workers),
            forwarders: Vec::new(),
        }
    }

    fn forward<T: Send + 'static>(
        &mut self,
        receiver: Receiver<T>,
        event: impl Fn(T) -> Vec<Event> + Send + 'static,
    ) {
        let sender = self.sender.clone();
        let wake = self.wake.clone();
        let closed = self.closed.clone();
        self.forwarders.push(thread::spawn(move || {
            while !closed.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(value) => {
                        for event in event(value) {
                            if sender.send(event).is_err() {
                                return;
                            }
                            wake();
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        }));
    }

    pub fn submit(&mut self, id: u64, mut command: Command) -> Result<(), String> {
        if let Command::RecordUsage(row) = command {
            self.usage.record(row);
            return Ok(());
        }
        if crate::remote::native::Binding::owner(&mut command)
            .and_then(|p| crate::remote::native::checkout_id(p))
            .is_some()
        {
            return Err("A VM checkout action requires its SSH connection.".into());
        }
        if self.closed.load(Ordering::Acquire) {
            return Err("Application is shutting down".into());
        }
        crate::diagnostics::command("submitted", id, &command);
        self.mailboxes.try_send(id, command).inspect_err(|_| {
            if let Some(mark) = crate::diagnostics::mark("request", "rejected", id, 0) {
                mark.record("rejected", 0);
            }
        })
    }

    pub fn watch_checkouts(&mut self, mut projects: Vec<String>) {
        projects.sort();
        projects.dedup();
        if projects != self.watched_projects {
            if let Some(watch) = &self.checkout_watch {
                let _ = watch.send(projects.clone());
            }
            self.watched_projects = projects;
        }
    }

    pub fn drain(&self) -> Vec<Event> {
        let events: Vec<_> = self
            .inbox
            .iter()
            .flat_map(|inbox| inbox.try_iter())
            .take(256)
            .collect();
        if events.len() == 256 {
            (self.wake)();
        }
        events
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.checkout_watch.take();
        self.inbox.take(); // Release any producers waiting on a full inbox.
        let workers = self.workers.take().map(worker::Workers::stop);
        if let Some(providers) = &self.providers {
            providers.shutdown();
        }
        for worker in self.forwarders.drain(..).chain(workers) {
            let _ = worker.join();
        }
    }
}
