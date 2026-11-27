use super::{Options, Provider, ProviderRuntime, Reply, SessionKey, Summary};
use crate::application::{Attachment, AttachmentKind, ExportFormat, ForkBoundary};

impl ProviderRuntime {
    pub(super) fn send_prompt(
        &self,
        key: &SessionKey,
        prompt: &str,
        options: &Options,
    ) -> Result<Reply, String> {
        let excerpt = crate::application::split_context(prompt).0;
        let checkpoint = crate::checkpoints::capture(&key.scope.project, excerpt)
            .ok()
            .flatten();
        let sent = match key.scope.provider {
            Provider::Codex => self
                .codex
                .send_prompt_with_permissions(
                    &key.scope.project,
                    &key.id,
                    prompt,
                    options.model.as_deref(),
                    options.effort.as_deref(),
                    options.permissions,
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
        };
        match sent {
            Ok(()) => Ok(Reply::Sent(checkpoint)),
            Err(error) => {
                if let Some(cp) = checkpoint {
                    crate::checkpoints::prune_ref(&key.scope.project, &cp.ref_name);
                }
                Err(error)
            }
        }
    }

    pub(super) fn checkpoint_files(
        &self,
        key: &SessionKey,
        from: &str,
        to: &str,
    ) -> Result<Reply, String> {
        if key.scope.provider == Provider::Codex {
            return crate::checkpoints::diff_since(&key.scope.project, from)
                .map(|diff| Reply::CheckpointFiles(diff.paths));
        }
        if key.scope.provider != Provider::OpenCode {
            return Err("File restoration is unavailable for this provider.".into());
        }
        self.opencode
            .checkpoint_files(&key.scope.project, &key.id, from, to)
            .map(Reply::CheckpointFiles)
            .map_err(|e| e.to_string())
    }

    pub(super) fn export(
        &self,
        key: &SessionKey,
        title: &str,
        format: ExportFormat,
        path: &std::path::Path,
    ) -> Result<(), String> {
        if key.scope.provider == Provider::Codex {
            self.codex
                .verify_export_owner(&key.scope.project, &key.id)
                .map_err(|e| e.to_string())?;
        }
        crate::application::export::save(key, title, format, path, |cursor| {
            self.history_page(key, cursor)
        })
    }

    pub(super) fn edit_branch(
        &self,
        key: &SessionKey,
        boundary: &ForkBoundary,
        text: &str,
        attachments: &[Attachment],
    ) -> Result<Reply, String> {
        let mut attachments = attachments.to_vec();
        for a in &mut attachments {
            if let Some(hash) = a
                .content
                .strip_prefix("opencode-media:")
                .filter(|_| a.kind == AttachmentKind::Image)
            {
                if key.scope.provider != Provider::OpenCode {
                    return Err("Unexpected provider media reference".into());
                }
                a.content = self
                    .opencode
                    .read_image(&key.scope.project, hash)
                    .map_err(|e| format!("Could not restore {}: {e}", a.path))?;
            }
        }
        let child = self.branch(key, boundary)?;
        Ok(Reply::EditableBranch {
            source: key.clone(),
            child,
            text: text.into(),
            attachments,
        })
    }
    pub(super) fn history_page(
        &self,
        key: &SessionKey,
        cursor: Option<&str>,
    ) -> Result<caelum_protocol::SessionHistoryPage, String> {
        match key.scope.provider {
            Provider::Codex => self
                .codex
                .session_history(&key.scope.project, &key.id, cursor)
                .map_err(|e| e.to_string()),
            Provider::OpenCode => self
                .opencode
                .session_history(&key.scope.project, &key.id, cursor)
                .map_err(|e| e.to_string()),
            Provider::Antigravity => self
                .antigravity
                .session_history(&key.scope.project, &key.id)
                .map_err(|e| e.to_string()),
        }
    }
    pub(super) fn fork(
        &self,
        source: &SessionKey,
        boundary: &ForkBoundary,
    ) -> Result<Reply, String> {
        self.branch(source, boundary).map(|child| Reply::Forked {
            source: source.clone(),
            child,
        })
    }

    fn branch(&self, source: &SessionKey, boundary: &ForkBoundary) -> Result<Summary, String> {
        let scope = &source.scope;
        let (id, title, updated_at, options) = match (scope.provider, boundary) {
            (Provider::Codex, ForkBoundary::CodexTurn(turn) | ForkBoundary::CodexBefore(turn)) => {
                let info = self
                    .codex
                    .fork_session(
                        &scope.project,
                        &source.id,
                        turn,
                        matches!(boundary, ForkBoundary::CodexBefore(_)),
                    )
                    .map_err(|e| e.to_string())?;
                (
                    info.thread.id,
                    info.thread.name.unwrap_or(info.thread.preview),
                    info.thread.updated_at.map(|t| t.saturating_mul(1000)),
                    Options {
                        model: info.model,
                        effort: info.effort,
                        agent: None,
                        permissions: None,
                    },
                )
            }
            (
                Provider::OpenCode,
                ForkBoundary::OpenCodeBefore(msg) | ForkBoundary::OpenCodeTip(msg),
            ) => {
                let s = self
                    .opencode
                    .fork_session(
                        &scope.project,
                        &source.id,
                        msg,
                        matches!(boundary, ForkBoundary::OpenCodeBefore(_)),
                    )
                    .map_err(|e| e.to_string())?
                    .session;
                (
                    s.id,
                    s.title.unwrap_or_default(),
                    s.updated_at,
                    Options {
                        model: s.model,
                        effort: s.effort,
                        agent: s.agent,
                        permissions: None,
                    },
                )
            }
            _ => return Err("Fork boundary belongs to a different provider".into()),
        };
        Ok(Summary {
            key: SessionKey {
                scope: scope.clone(),
                id,
            },
            title,
            updated_at,
            options,
            working: false,
        })
    }
}
