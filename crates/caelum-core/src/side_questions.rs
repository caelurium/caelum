//! Independent, bounded side turns. Only visible messages from the parent are shared.
use crate::application::{Conversation, EntryKind, Event, Provider, ProviderRuntime, SessionKey};
use std::{
    path::Path,
    process::Command,
    sync::{mpsc::SyncSender, Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::watch;

mod acp;
mod codex;
#[cfg(test)]
mod tests;
mod wire;

const LIMIT: usize = 64 * 1024;
#[derive(Debug, Clone)]
pub struct Turn {
    pub(crate) generation: u64,
    pub(crate) text: String,
    pub(crate) model: Option<String>,
    pub(crate) until: watch::Receiver<()>,
}

type Destination = (SyncSender<Event>, Arc<dyn Fn() + Send + Sync>);
#[derive(Clone, Default)]
pub(crate) struct Output(Arc<OnceLock<Destination>>);
impl Output {
    pub(crate) fn connect(&self, sender: SyncSender<Event>, wake: Arc<dyn Fn() + Send + Sync>) {
        let _ = self.0.set((sender, wake));
    }
}

pub(crate) fn prompt(history: &Conversation, question: &str) -> Result<String, String> {
    if question.len() > 128 * 1024 {
        return Err("The side question and its attachments exceed 128 KiB.".into());
    }
    let mut messages = Vec::new();
    for e in history
        .entries
        .iter()
        .rev()
        .filter(|e| matches!(e.kind(), EntryKind::User | EntryKind::Assistant))
        .take(12)
    {
        let excerpt = &e.text[..e.text.floor_char_boundary(e.text.len().min(2048))];
        let body = excerpt.split("\n\n<caelum-").next().unwrap_or_default();
        let text = crate::application::presentation::visible_text(body);
        if !text.trim().is_empty() {
            messages.push((
                if e.kind() == EntryKind::User {
                    "user"
                } else {
                    "assistant"
                },
                text,
            ));
        }
    }
    messages.reverse();
    Ok(format!("Answer this side question using the conversation excerpt. Do not make changes.\n\nRecent messages (context only):\n{}\n\nSide question:\n{question}",serde_json::to_string(&messages).map_err(|e| e.to_string())?))
}

pub(crate) fn run(
    runtime: &ProviderRuntime,
    key: &SessionKey,
    turn: &Turn,
    output: &Output,
) -> Result<String, String> {
    if turn.until.has_changed().is_err() {
        return Err("Side question closed.".into());
    }
    let scratch;
    let profile;
    let (cmd, root) = match key.scope.provider {
        Provider::Codex => (
            runtime.codex.side_command(&key.scope.project, &key.id)?,
            Path::new(&key.scope.project),
        ),
        Provider::Antigravity => {
            let mut cmd = runtime
                .antigravity
                .side_command(&key.scope.project, &key.id)?;
            scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
            let home = std::env::var_os("HOME").ok_or("Missing Antigravity home directory.")?;
            profile = tempfile::Builder::new()
                .prefix("caelum-side-")
                .tempdir_in(Path::new(&home).join(".gemini"))
                .map_err(|e| e.to_string())?;
            cmd.arg("--app-data-dir")
                .arg(profile.path().file_name().unwrap());
            cmd.current_dir(scratch.path());
            (cmd, scratch.path())
        }
        Provider::OpenCode => return Err("OpenCode uses its session generation endpoint.".into()),
    };
    execute(&runtime.processes, cmd, root, key, turn, output)
}

fn execute(
    processes: &crate::processes::ProcessRegistry,
    mut cmd: Command,
    root: &Path,
    key: &SessionKey,
    turn: &Turn,
    output: &Output,
) -> Result<String, String> {
    let io = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let mut until = turn.until.clone();
    io.block_on(async {
        let mut wire = wire::Wire::start(&mut cmd,processes)?;
        let mut last = None;
        let mut publish = |text:&str| {
            if let Some((tx,wake))=output.0.get() {
                if last.is_none_or(|t:Instant| t.elapsed()>=Duration::from_millis(60))
                    && tx.try_send(Event::SideAnswer(key.clone(),turn.generation,text.into())).is_ok() {
                    last=Some(Instant::now()); wake();
                }
            }
        };
        tokio::select! {
            biased;
            _ = until.changed() => Err("Side question closed.".into()),
            result = tokio::time::timeout(Duration::from_secs(120),async {
                match key.scope.provider {
                    Provider::Codex => codex::ask(&mut wire,root,&key.id,turn,&mut publish).await,
                    Provider::Antigravity => acp::ask(&mut wire,root,&key.id,turn,&mut publish).await,
                    Provider::OpenCode => unreachable!()
                }
            }) => result.map_err(|_|"Side question timed out.".to_owned())?,
        }
    })
}

fn append(answer: &mut String, delta: &str) -> Result<(), String> {
    if delta.len() > LIMIT - answer.len() {
        return Err("Side answer exceeds 64 KiB.".into());
    }
    answer.push_str(delta);
    Ok(())
}

fn answered(answer: String) -> Result<String, String> {
    if answer.trim().is_empty() {
        Err("The provider returned no side answer.".into())
    } else {
        Ok(answer)
    }
}
