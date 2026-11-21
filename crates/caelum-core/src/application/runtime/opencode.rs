use super::{OpenCodeMove, Options, Provider, ProviderRuntime, Reply, SessionKey, Summary};
use crate::opencode::OpenCodeError;

impl ProviderRuntime {
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
