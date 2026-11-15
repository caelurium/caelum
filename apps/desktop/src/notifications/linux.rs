//! Freedesktop notifications. All bus I/O and signal waiting stay off Rune's thread.
use std::{collections::BTreeMap, thread, time::Duration};

use caelum_core::application::{Notification, Reply, SessionKey};
use futures_util::StreamExt;
use rune::AppHandle;
use tokio::sync::mpsc;
use zbus::{Connection, Proxy, zvariant::Value};

use crate::Workspace;

const LIMIT: usize = 64;
const TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct Client(mpsc::Sender<(u64, Notification)>);

impl Client {
    pub(super) fn new(handle: AppHandle<Workspace>) -> Result<Self, String> {
        let (sender, mut receiver) = mpsc::channel(LIMIT);
        thread::Builder::new()
            .name("desktop-notifications".into())
            .spawn(move || {
                // Do not connect to desktop services until there is an actual notification.
                while let Some(first) = receiver.blocking_recv() {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| error.to_string())
                        .and_then(|runtime| runtime.block_on(run(&handle, &mut receiver, first)));
                    if let Err(error) = result {
                        eprintln!("Notification: {error}");
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self(sender))
    }

    pub(super) fn send(&self, id: u64, notification: Notification) -> Result<(), String> {
        self.0
            .try_send((id, notification))
            .map_err(|error| format!("Notification queue: {error}"))
    }
}

async fn run(
    handle: &AppHandle<Workspace>,
    receiver: &mut mpsc::Receiver<(u64, Notification)>,
    first: (u64, Notification),
) -> Result<(), String> {
    let service = tokio::time::timeout(TIMEOUT, async {
        let connection = Connection::session().await?;
        let proxy = Proxy::new_owned(
            connection,
            "org.freedesktop.Notifications",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications",
        )
        .await?;
        let actions = proxy.receive_signal("ActionInvoked").await?;
        let closed = proxy.receive_signal("NotificationClosed").await?;
        Ok::<_, zbus::Error>((proxy, actions, closed))
    })
    .await
    .map_err(|error| error.to_string())
    .and_then(|result| result.map_err(|error| error.to_string()));
    let (proxy, mut actions, mut closed) = match service {
        Ok(service) => service,
        Err(error) => {
            super::complete(handle, first.0, Err(error.clone()));
            return Err(error);
        }
    };
    let mut targets = BTreeMap::new();
    deliver(handle, &proxy, &mut targets, first).await;
    loop {
        tokio::select! {
            request = receiver.recv() => match request {
                Some(request) => deliver(handle, &proxy, &mut targets, request).await,
                None => return Ok(()),
            },
            message = actions.next() => {
                let Some(message) = message else { return Err("Notification action stream closed".into()); };
                if let Ok((id, action)) = message.body().deserialize::<(u32, String)>()
                    && action == "default"
                    && let Some(target) = targets.remove(&id)
                {
                    handle.update(move |s, _| super::clicked(s, target));
                }
            },
            message = closed.next() => {
                let Some(message) = message else { return Err("Notification close stream closed".into()); };
                if let Ok((id, _reason)) = message.body().deserialize::<(u32, u32)>() {
                    targets.remove(&id);
                }
            },
        }
    }
}

async fn deliver(
    handle: &AppHandle<Workspace>,
    proxy: &Proxy<'_>,
    targets: &mut BTreeMap<u32, Option<SessionKey>>,
    (request, notification): (u64, Notification),
) {
    if targets.len() >= LIMIT
        && let Some((id, _)) = targets.pop_first()
    {
        let _ = tokio::time::timeout(TIMEOUT, proxy.call::<_, _, ()>("CloseNotification", &(id,)))
            .await;
    }
    let hints = std::collections::HashMap::from([
        ("desktop-entry", Value::from("dev.caelum.desktop")),
        ("suppress-sound", Value::from(true)),
    ]);
    let body = escape_markup(&notification.body);
    let args = (
        "Caelum",
        0u32,
        "dev.caelum.desktop",
        &notification.title,
        body,
        vec!["default", "Open Caelum"],
        hints,
        10_000i32,
    );
    let result = tokio::time::timeout(TIMEOUT, proxy.call::<_, _, u32>("Notify", &args))
        .await
        .map_err(|error| error.to_string())
        .and_then(|result| result.map_err(|error| error.to_string()));
    let result = result.map(|id| {
        targets.insert(id, notification.target);
        Reply::Done
    });
    super::complete(handle, request, result);
}

fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
