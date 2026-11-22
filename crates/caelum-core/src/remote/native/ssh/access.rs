use crate::remote::boat::checkout_quote as quote;
use crate::{database::WorkspaceDatabase, github::cache::Cache, remote::native::NativeTarget};
use std::{
    collections::HashMap,
    future::Future,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::Command, runtime::Handle};

pub(crate) struct Metadata {
    pub(crate) branches: WorkspaceDatabase,
    target: NativeTarget,
    parent: PathBuf,
    cache: Cache,
}
impl Metadata {
    pub(super) fn open(target: &NativeTarget, parent: &Path, store: &Path) -> Result<Self, String> {
        Ok(Self {
            branches: WorkspaceDatabase::open(store.with_file_name("branches.sqlite3"))
                .map_err(|e| e.to_string())?,
            target: target.clone(),
            parent: parent.into(),
            cache: Cache::for_store(store),
        })
    }
    pub(crate) fn review<T>(&self, f: impl FnOnce(&Cache) -> T) -> Option<T> {
        self.target
            .with_live_profile(&self.parent, || Ok(f(&self.cache)))
            .ok()
    }
}

/// Operations can only use this authenticated master; OpenSSH must not fall back
/// to a fresh connection when its control socket disappears.
pub(crate) struct Access {
    socket: PathBuf,
    host: String,
    forwards: std::sync::Arc<tokio::sync::Mutex<HashMap<u16, u16>>>,
    pub(crate) root: String,
    pub(crate) io: Handle,
    pub(crate) metadata: Option<Metadata>,
    pub(crate) codex_used: std::sync::atomic::AtomicBool,
    pub(crate) live: std::sync::Arc<std::sync::atomic::AtomicBool>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl Access {
    pub(crate) fn new(socket: PathBuf, host: String, root: String) -> Self {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            socket,
            host,
            forwards: Default::default(),
            root,
            io: Handle::current(),
            metadata: None,
            codex_used: Default::default(),
            live: std::sync::Arc::new(true.into()),
            shutdown,
        }
    }
    pub(in crate::remote::native) fn channel(&self, options: &[&str]) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.args(["-F", "/dev/null", "-S"])
            .arg(&self.socket)
            .args(["-o", "ProxyCommand=false", "-o", "BatchMode=yes", "-T"])
            .args(options)
            .arg("--")
            .arg(&self.host)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }

    pub(crate) fn close(&self) {
        self.live.store(false, std::sync::atomic::Ordering::Release);
        self.shutdown.send_replace(true);
    }

    pub(in crate::remote::native) async fn bounded<T>(
        &self,
        duration: Duration,
        operation: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let mut closing = self.shutdown.subscribe();
        if *closing.borrow() {
            return Err("The VM connection closed.".into());
        }
        tokio::select! {
            _ = closing.changed() => Err("The VM connection closed.".into()),
            result = tokio::time::timeout(duration, operation) => {
                result.unwrap_or_else(|_| Err("VM file access timed out.".into()))
            }
        }
    }

    pub(in crate::remote::native) async fn run(
        &self,
        script: &str,
        limit: usize,
        codes: &[i32],
    ) -> Result<Vec<u8>, String> {
        if !self.live.load(std::sync::atomic::Ordering::Acquire) {
            return Err("The VM connection closed.".into());
        }
        let mut child = self
            .channel(&[])
            .arg(format!("sh -c {} caelum-vm", quote(script)))
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let mut closing = self.shutdown.subscribe();
        if *closing.borrow() {
            return Err("The VM connection closed.".into());
        }
        tokio::select! {
            _ = closing.changed() => Err("The VM connection closed.".into()),
            result = tokio::time::timeout(Duration::from_secs(35), async {
            let mut data = Vec::new();
            let mut stdout = stdout.take(limit as u64 + 1);
            let diagnostics = tokio::spawn(read_diagnostic(stderr));
            if let Err(error) = stdout.read_to_end(&mut data).await {
                let _ = child.kill().await;
                return Err(error.to_string());
            }
            if data.len() > limit {
                let _ = child.kill().await;
                return Err("VM output exceeds the preview limit.".into());
            }
            let (diagnostics, status) = tokio::try_join!(
                async {
                    diagnostics
                        .await
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())
                },
                async { child.wait().await.map_err(|e| e.to_string()) },
            )?;
            if !status.code().is_some_and(|c| codes.contains(&c)) {
                let detail = diagnostic(&diagnostics);
                return Err(if detail.is_empty() {
                    format!("VM command failed ({status}).")
                } else {
                    format!("VM command failed ({status}): {detail}")
                });
            }
            Ok(data)
            }) => result.map_err(|_| String::from("VM command timed out."))?
        }
    }

    async fn forward(&self, remote: u16) -> Result<u16, String> {
        let mut forwards = self.forwards.lock().await;
        if let Some(port) = forwards.get(&remote) {
            return Ok(*port);
        }
        if forwards.len() >= 8 {
            return Err("The Codex listener has changed repeatedly. Reconnect to the VM.".into());
        }
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .map_err(|e| e.to_string())?;
        let local = listener.local_addr().map_err(|e| e.to_string())?.port();
        forwards.insert(remote, local);
        let socket = self.socket.clone();
        let host = self.host.clone();
        let shutdown = self.shutdown.clone();
        let live = self.live.clone();
        let forwards = self.forwards.clone();
        tokio::spawn(async move {
            let mut closing = shutdown.subscribe();
            let (failed, mut failures) = tokio::sync::mpsc::unbounded_channel();
            let (retired, _) = tokio::sync::watch::channel(false);
            loop {
                if *closing.borrow() {
                    break;
                }
                tokio::select! {
                    _ = closing.changed() => break,
                    Some(()) = failures.recv() => break,
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) if live.load(std::sync::atomic::Ordering::Acquire) => {
                            let socket = socket.clone();
                            let host = host.clone();
                            let shutdown = shutdown.clone();
                            let failed = failed.clone();
                            let retired = retired.clone();
                            tokio::spawn(async move {
                                let mut command = Command::new("ssh");
                                command
                                    .args(["-F", "/dev/null", "-S"])
                                    .arg(socket)
                                    .args(["-o", "ProxyCommand=false", "-o", "BatchMode=yes", "-T", "-W"])
                                    .arg(format!("127.0.0.1:{remote}"))
                                    .arg("--")
                                    .arg(host)
                                    .stdin(Stdio::piped())
                                    .stdout(Stdio::piped())
                                    .stderr(Stdio::null())
                                    .kill_on_drop(true);
                                let Ok(mut child) = command.spawn() else {
                                    let _ = failed.send(());
                                    return;
                                };
                                let (Some(mut stdin), Some(mut stdout)) =
                                    (child.stdin.take(), child.stdout.take())
                                else {
                                    return;
                                };
                                let (mut reader, mut writer) = tokio::io::split(stream);
                                let mut closing = shutdown.subscribe();
                                let mut retiring = retired.subscribe();
                                if *closing.borrow() || *retiring.borrow() {
                                    let _ = child.kill().await;
                                    return;
                                }
                                let copy = async {
                                    tokio::try_join!(
                                        async {
                                            tokio::io::copy(&mut reader, &mut stdin).await?;
                                            tokio::io::AsyncWriteExt::shutdown(&mut stdin).await
                                        },
                                        async {
                                            tokio::io::copy(&mut stdout, &mut writer).await
                                        }
                                    )
                                };
                                tokio::select! {
                                    _ = closing.changed() => {
                                        let _ = child.kill().await;
                                    },
                                    _ = retiring.changed() => {
                                        let _ = child.kill().await;
                                    },
                                    _ = child.wait() => {
                                        let _ = failed.send(());
                                    },
                                    result = copy => {
                                        if result.is_err() {
                                            let _ = child.kill().await;
                                        } else if !matches!(
                                            tokio::time::timeout(Duration::from_secs(3), child.wait()).await,
                                            Ok(Ok(status)) if status.success()
                                        ) {
                                            let _ = failed.send(());
                                            let _ = child.kill().await;
                                        }
                                    }
                                }
                            });
                        }
                        Ok(_) => break,
                        Err(_) => break,
                    }
                }
            }
            retired.send_replace(true);
            let mut forwards = forwards.lock().await;
            if forwards.get(&remote) == Some(&local) {
                forwards.remove(&remote);
            }
        });
        Ok(local)
    }

    pub(crate) fn codex(&self) -> Result<String, String> {
        let cmd = format!(
            "cd -P {} && sh -c {}",
            quote(&self.root),
            quote(include_str!("codex.sh"))
        );
        let bytes = self.io.block_on(self.run(&cmd, 16, &[0]))?;
        let remote = std::str::from_utf8(&bytes)
            .ok()
            .and_then(|s| s.trim().parse::<u16>().ok())
            .filter(|p| *p > 0)
            .ok_or("Invalid Codex listener port.")?;
        let port = self.io.block_on(self.forward(remote))?;
        Ok(format!("ws://127.0.0.1:{port}"))
    }

    pub(crate) fn gh<'a>(
        &self,
        args: impl Iterator<Item = &'a std::ffi::OsStr>,
        timeout: Duration,
    ) -> Result<Vec<u8>, String> {
        let args = args
            .map(|a| a.to_str().map(quote).ok_or("Invalid gh argument"))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        let script=format!("cd -P {} && unset GH_REPO && PATH=\"$HOME/.local/bin:$HOME/.nix-profile/bin:$HOME/.opencode/bin:$HOME/.npm-global/bin:$HOME/.volta/bin:$HOME/.bun/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:$HOME/.asdf/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:$PATH\" GH_PROMPT_DISABLED=1 GH_SPINNER_DISABLED=1 GH_PAGER=cat GH_NO_UPDATE_NOTIFIER=1 gh {args}",quote(&self.root));
        self.io.block_on(async {
            tokio::time::timeout(timeout, self.run(&script, 1024 * 1024, &[0]))
                .await
                .map_err(|_| "GitHub timed out on the VM.")?
        })
    }
}

fn diagnostic(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .take(512)
        .collect::<String>()
        .trim()
        .to_owned()
}

async fn read_diagnostic(
    mut reader: impl tokio::io::AsyncRead + Unpin,
) -> std::io::Result<Vec<u8>> {
    let mut saved = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(saved);
        }
        let remaining = 8192usize.saturating_sub(saved.len());
        saved.extend_from_slice(&buffer[..read.min(remaining)]);
    }
}
