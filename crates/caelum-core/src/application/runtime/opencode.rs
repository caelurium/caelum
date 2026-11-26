use super::{OpenCodeMove, Options, Provider, ProviderRuntime, Reply, SessionKey, Summary};
use crate::opencode::OpenCodeError;

impl ProviderRuntime {
    pub(super) fn run_opencode_command(
        &self,
        key: &SessionKey,
        prompt: &str,
        options: &Options,
    ) -> Result<(), String> {
        let command = crate::application::composer::command_text(prompt);
        let input = command
            .trim_start()
            .strip_prefix('/')
            .filter(|s| !s.starts_with('/'))
            .filter(|_| key.scope.provider == Provider::OpenCode)
            .ok_or("Invalid OpenCode command")?;
        let (name, text) = input.split_once(char::is_whitespace).unwrap_or((input, ""));
        if let Some(agent) = &options.agent {
            self.opencode
                .select_agent(&key.scope.project, &key.id, agent)
                .map_err(|e| e.to_string())?;
        }
        self.opencode
            .run_command(
                &key.scope.project,
                &key.id,
                &caelum_protocol::OpenCodeCommandInput {
                    name: name.into(),
                    text: text.into(),
                    files: crate::application::composer::command_files(prompt),
                },
                options.model.as_deref(),
                options.effort.as_deref(),
            )
            .map_err(|e| e.to_string())
    }

    pub(super) fn verify_opencode_move(
        &self,
        from: &SessionKey,
        destination: &str,
    ) -> Result<Reply, String> {
        if from.scope.provider != Provider::OpenCode {
            return Err("Invalid OpenCode relocation target".into());
        }

        let (project, archived) = self
            .opencode
            .session_location(destination, &from.id)
            .map_err(|e| e.to_string())?;
        if project.path != destination {
            return Ok(Reply::Done);
        }
        if from.scope.project != destination {
            self.opencode
                .forget_previous_location(&from.scope.project, &from.id);
        }
        Ok(Reply::SessionMoved(OpenCodeMove {
            from: from.clone(),
            project,
            archived,
            linked_worktree: false,
        }))
    }

    pub(super) fn resume_opencode(&self, key: &SessionKey) -> Result<Reply, String> {
        match self
            .opencode
            .resume_with_history(&key.scope.project, &key.id)
        {
            Ok((info, page)) => Ok(Reply::Resumed(
                Summary {
                    key: key.clone(),
                    title: info.session.title.unwrap_or_default(),
                    updated_at: info.session.updated_at,
                    working: info.session.is_working,
                    options: Options {
                        model: info.session.model,
                        effort: info.session.effort,
                        agent: info.session.agent,
                        permissions: None,
                    },
                },
                page,
            )),
            Err(OpenCodeError::SessionNotInProject) => {
                let (project, archived) = self
                    .opencode
                    .session_location(&key.scope.project, &key.id)
                    .map_err(|e| e.to_string())?;

                if project.path == key.scope.project {
                    return Err("OpenCode session moved during refresh; refresh again".into());
                }
                let linked_worktree = self
                    .worktrees
                    .list(&key.scope.project, "")
                    .map_err(|e| e.to_string())?
                    .is_some_and(|listing| {
                        listing
                            .worktrees
                            .iter()
                            .any(|tree| tree.available && tree.project.path == project.path)
                    });

                Ok(Reply::SessionMoved(OpenCodeMove {
                    from: key.clone(),
                    project,
                    archived,
                    linked_worktree,
                }))
            }
            Err(error) => Err(error.to_string()),
        }
    }
}
