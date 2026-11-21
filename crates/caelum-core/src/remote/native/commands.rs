use crate::application::{Command, Provider, Task};

/// Commands owned by this exact VM checkout.
pub(super) fn admitted(command: &Command, root: &str) -> bool {
    let project = match command {
        Command::Open(path) => path,
        Command::List(s)
        | Command::ListArchived(s)
        | Command::Choices(s)
        | Command::Start(s, _)
            if s.provider == Provider::Codex =>
        {
            &s.project
        }
        Command::Rename(k, _)
        | Command::Rewind { key: k, .. }
        | Command::Archive(k, _)
        | Command::DeleteSession(k)
        | Command::Details(k)
        | Command::Fork(k, _)
        | Command::EditBranch(k, ..)
        | Command::Resume(k)
        | Command::History(k, _)
        | Command::CodexControlHistory(k, _)
        | Command::Send(k, ..)
        | Command::Cancel(k)
            if k.scope.provider == Provider::Codex =>
        {
            &k.scope.project
        }
        Command::Approve(p, _) if p.provider == Provider::Codex.as_str() => &p.project_path,
        Command::Answer(p, _) if p.provider == Provider::Codex.as_str() => &p.project_path,
        Command::Task(Task::Image { project, source })
            if !source.starts_with("opencode-media:") =>
        {
            project
        }
        Command::Task(
            t @ (Task::EncodeImage { .. }
            | Task::Completions { .. }
            | Task::Directory { .. }
            | Task::File { .. }
            | Task::Search { .. }
            | Task::Git { .. }
            | Task::Diff { .. }
            | Task::Discover { .. }
            | Task::Details { .. }
            | Task::ReviewCache { .. }
            | Task::ReviewFiles { .. }
            | Task::Feedback { .. }
            | Task::AttachPullRequest { .. }
            | Task::Merge { .. }
            | Task::SetAutoMerge { .. }
            | Task::UpdateBranch { .. }
            | Task::Worktrees { .. }),
        ) => t.project(),
        _ => return false,
    };
    project == root
}
