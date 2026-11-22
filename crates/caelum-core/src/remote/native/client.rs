use super::{commands, ssh, NativeTarget};
use crate::application::{Command, Event, ProviderRuntime, Runtime};
use std::{collections::BTreeSet, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch, Notify};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Status {
    Connecting,
    Connected,
    Disconnected(String),
}

pub enum Notice {
    Status(Status),
    Event(Box<Event>),
}

type Link = mpsc::Sender<(u64, Command)>;
enum Message {
    Connected(Link),
    Notice(Notice),
}
struct ActiveAttempt {
    pending: BTreeSet<u64>,
    requests: mpsc::Receiver<(u64, Command)>,
}

enum Attempt {
    Connecting,
    Active(ActiveAttempt),
}

impl Attempt {
    fn new() -> Self {
        Self::Connecting
    }

    fn activate(&mut self) -> (&mut ActiveAttempt, Link) {
        let (writer, requests) = mpsc::channel(128);
        *self = Self::Active(ActiveAttempt {
            pending: BTreeSet::new(),
            requests,
        });
        let Self::Active(active) = self else {
            unreachable!();
        };
        (active, writer)
    }

    fn retire(self) -> Option<BTreeSet<u64>> {
        let Self::Active(mut active) = self else {
            return None;
        };
        active.requests.close();
        while let Ok((id, _)) = active.requests.try_recv() {
            active.pending.insert(id);
        }
        Some(active.pending)
    }
}

enum BrokerSync {
    Pending,
    Attached,
}
struct Output {
    tx: mpsc::Sender<Message>,
    wake: Arc<dyn Fn() + Send + Sync>,
}
impl Output {
    async fn send(&self, n: Notice) {
        self.publish(Message::Notice(n)).await;
    }
    async fn publish(&self, message: Message) {
        if self.tx.send(message).await.is_ok() {
            (self.wake)()
        }
    }
}

/// A connection accepts commands only after SSH verifies the checkout. No outbox
/// survives a tunnel loss. An uncertain mutation is reconciled from native history, never retried.
pub struct Client {
    root: String,
    /// Only the connection whose Connected notice the desktop has observed can accept commands.
    link: Option<Link>,
    rx: mpsc::Receiver<Message>,
    retry: mpsc::Sender<()>,
    stop: watch::Sender<bool>,
    wake: Arc<dyn Fn() + Send + Sync>,
    ssh: Option<std::thread::JoinHandle<()>>,
}

impl Client {
    pub fn start(
        target: NativeTarget,
        parent_store: PathBuf,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self, String> {
        let root = target.root()?;
        Self::with_tunnel(root, wake, async move || {
            ssh::open(&target, &parent_store).await
        })
    }

    pub(super) fn with_tunnel(
        root: String,
        wake: impl Fn() + Send + Sync + 'static,
        mut open: impl AsyncFnMut() -> Result<ssh::Tunnel, String> + Send + 'static,
    ) -> Result<Self, String> {
        Self::with_connection(root, wake, async move |attempt, out| {
            let mut tunnel = match open().await {
                Ok(tunnel) => tunnel,
                Err(e) => return e,
            };
            tokio::select! {
                _=tunnel.child.wait()=>"SSH disconnected. Reconnect to verify the VM’s address and host key.".to_owned(),
                e=session(tunnel.access.clone(),attempt,out)=>e,
            }
        })
    }

    fn with_connection(
        root: String,
        wake: impl Fn() + Send + Sync + 'static,
        connect: impl AsyncFnMut(&mut Attempt, &Output) -> String + Send + 'static,
    ) -> Result<Self, String> {
        let (tx, rx) = mpsc::channel(512);
        let (retry, again) = mpsc::channel(1);
        let (stop, mut closing) = watch::channel(false);
        let wake = Arc::new(wake);
        let out = Output {
            tx,
            wake: wake.clone(),
        };
        let ssh = std::thread::Builder::new()
            .name("native-workspace-ssh".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match rt {
                    Ok(rt) => rt.block_on(async {
                        tokio::select! {
                            _ = closing.changed()=>{},
                            _ = supervise(connect,again,&out)=>{},
                        }
                    }),
                    Err(e) => {
                        let _ = out.tx.blocking_send(Message::Notice(Notice::Status(
                            Status::Disconnected(e.to_string()),
                        )));
                        (out.wake)();
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            link: None,
            rx,
            retry,
            stop,
            wake,
            ssh: Some(ssh),
        })
    }

    pub fn submit(&self, id: u64, command: Command) -> Result<(), String> {
        if !commands::admitted(&command, &self.root) {
            return Err("This action is not available in a native VM workspace.".into());
        }
        let offline = "The VM is disconnected. Reconnect before trying again.";
        let writer = self.link.as_ref().ok_or(offline)?;
        writer
            .try_send((id, command))
            .map_err(|_| "The VM connection is busy or closed. Try again.".into())
    }
    pub fn reconnect(&self) {
        let _ = self.retry.try_send(());
    }
    pub fn drain(&mut self) -> Vec<Notice> {
        let notices: Vec<_> = std::iter::from_fn(|| self.rx.try_recv().ok())
            .take(256)
            .map(|m| match m {
                Message::Connected(link) => {
                    self.link = Some(link);
                    Notice::Status(Status::Connected)
                }
                Message::Notice(n) => {
                    if matches!(n, Notice::Status(_)) {
                        self.link = None;
                    }
                    n
                }
            })
            .collect();
        if notices.len() == 256 {
            (self.wake)();
        }
        notices
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.rx.close();
        self.stop.send_replace(true);
        if let Some(ssh) = self.ssh.take() {
            let _ = ssh.join();
        }
    }
}

async fn supervise(
    mut connect: impl AsyncFnMut(&mut Attempt, &Output) -> String,
    mut again: mpsc::Receiver<()>,
    out: &Output,
) {
    loop {
        out.send(Notice::Status(Status::Connecting)).await;
        let mut attempt = Attempt::new();
        let problem = connect(&mut attempt, out).await;
        let retired = attempt.retire();
        out.send(Notice::Status(Status::Disconnected(problem)))
            .await;
        if let Some(pending) = retired {
            for id in pending {
                let error = "VM connection lost. Delivery is unconfirmed; check the session history before retrying.";
                out.send(Notice::Event(Box::new(Event::Completed(
                    id,
                    Err(error.into()),
                ))))
                .await;
            }
            // Try a fresh, independently authenticated connection once; a stopped VM or revoked
            // credential then waits for the user's Reconnect action rather than polling Boat.
            tokio::select! { _=tokio::time::sleep(Duration::from_secs(1))=>{}, _=again.recv()=>{} }
        } else if again.recv().await.is_none() {
            return;
        }
    }
}

// Runtime shutdown joins blocking provider workers; a dropped SSH attempt must
// release its master immediately, including while a read is still in progress.
struct Workers(Option<Runtime>, Arc<super::Access>);
impl Drop for Workers {
    fn drop(&mut self) {
        self.1.close();
        if let Some(runtime) = self.0.take() {
            std::thread::spawn(move || drop(runtime));
        }
    }
}
async fn deliver(pending: &mut BTreeSet<u64>, out: &Output, event: Event) {
    let completion = if let Event::Completed(id, _) = &event {
        if !pending.contains(id) {
            return;
        }
        Some(*id)
    } else {
        None
    };
    out.send(Notice::Event(Box::new(event))).await;
    if let Some(id) = completion {
        pending.remove(&id);
    }
}
async fn session(access: Arc<super::Access>, attempt: &mut Attempt, out: &Output) -> String {
    let wake = Arc::new(Notify::new());
    let notify = wake.clone();
    let providers = ProviderRuntime::forwarded(access.clone());
    let mut workers = Workers(
        Some(Runtime::with_providers(providers.clone(), move || {
            notify.notify_one()
        })),
        access.clone(),
    );
    let rt = workers.0.as_mut().unwrap();
    let mut brokers = BrokerSync::Pending;
    let (active, writer) = attempt.activate();
    out.publish(Message::Connected(writer)).await;
    loop {
        tokio::select! {
            _ = wake.notified() => for event in rt.drain() {
                // An empty, newly constructed broker isn't a snapshot of the VM.
                // Keep visible questions and answer drafts until Codex has resubscribed.
                if matches!(brokers, BrokerSync::Pending)
                    && access.codex_used.load(std::sync::atomic::Ordering::Acquire)
                {
                    brokers = BrokerSync::Attached;
                    deliver(&mut active.pending, out, Event::Approvals(providers.approvals.pending())).await;
                    deliver(&mut active.pending, out, Event::Questions(providers.questions.pending())).await;
                }
                let event = match event {
                    Event::Codex(ref e) if e.method == "codex/processExited" => {
                        return "VM Codex WebSocket disconnected.".into()
                    }
                    Event::Approvals(_) | Event::Questions(_) if matches!(brokers, BrokerSync::Pending) => continue,
                    Event::Approvals(_) => Event::Approvals(providers.approvals.pending()),
                    Event::Questions(_) => Event::Questions(providers.questions.pending()),
                    e => e,
                };
                deliver(&mut active.pending, out, event).await;
            },
            req = active.requests.recv() => match req {
                Some((id, cmd)) => {
                    if active.pending.len() >= 512 || !active.pending.insert(id) {
                        out.send(Notice::Event(Box::new(Event::Completed(
                            id,
                            Err("Too many pending native workspace requests.".into()),
                        ))))
                        .await;
                    } else if let Err(e) = rt.submit(id, cmd) {
                        deliver(&mut active.pending, out, Event::Completed(id, Err(e))).await;
                    }
                }
                None => return "VM workspace closed.".into(),
            }
        }
    }
}

#[cfg(test)]
mod tests;
