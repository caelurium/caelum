use super::{download, installation, Installed, Progress};
use caelum_protocol::UpdateCheck;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Debug, Clone)]
pub struct Job {
    pub id: u64,
    pub(super) action: Action,
    pub(super) cancel: Arc<Cancellation>,
}

#[derive(Debug, Default)]
pub(super) struct Cancellation {
    flag: AtomicBool,
    wake: tokio::sync::Notify,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Release);
        self.wake.notify_waiters();
    }
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
    pub async fn cancelled(&self) {
        let notified = self.wake.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if !self.is_cancelled() {
            notified.await;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Action {
    Download(UpdateCheck),
    Install(Arc<installation::Prepared>),
    Discard(Arc<installation::Prepared>),
    Finish,
}

#[derive(Debug)]
pub enum Reply {
    Prepared(Arc<installation::Prepared>),
    Installed(Arc<Installed>),
    Finished(Option<String>),
    Discarded,
}

type Observer = Arc<dyn Fn(u64, Progress) + Send + Sync>;

#[derive(Clone, Default)]
pub(crate) struct Backend {
    closed: Arc<Cancellation>,
    observer: Arc<Mutex<Option<Observer>>>,
}

impl Backend {
    pub fn observe(
        &self,
        sender: std::sync::mpsc::SyncSender<crate::application::Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) {
        let observer = move |id, progress| {
            if sender
                .try_send(crate::application::Event::UpdateProgress(id, progress))
                .is_ok()
            {
                wake();
            }
        };
        *self.observer.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(observer));
    }

    pub fn shutdown(&self) {
        self.closed.cancel();
    }

    pub fn execute(&self, job: &Job) -> Result<Reply, String> {
        let cancelled = || self.closed.is_cancelled() || job.cancel.is_cancelled();
        let observer = self
            .observer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let progress = |progress| {
            if let Some(observer) = &observer {
                observer(job.id, progress);
            }
        };
        match &job.action {
            Action::Download(release) => {
                download::prepare(release, &job.cancel, &self.closed, progress)
                    .map(Arc::new)
                    .map(Reply::Prepared)
            }
            Action::Install(prepared) => {
                if cancelled() {
                    return Err("Update cancelled".into());
                }
                installation::install(prepared.clone())
                    .map(Arc::new)
                    .map(Reply::Installed)
            }
            Action::Discard(prepared) => prepared.discard().map(|()| Reply::Discarded),
            Action::Finish => installation::finish().map(Reply::Finished),
        }
    }
}
