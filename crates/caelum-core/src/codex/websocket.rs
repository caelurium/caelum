//! One JSON-RPC message per text frame; the provider's existing request IDs,
//! events, approvals and questions are independent of its local/remote transport.
use super::{
    transport::{Messages, Writer},
    CodexError,
};
use crate::remote::native::Access;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

pub(super) type Outgoing = (String, std::sync::mpsc::Sender<Result<(), String>>);

pub(super) fn open(access: &Access, url: &str) -> Result<(Writer, Messages), CodexError> {
    let (mut ws, _) = access
        .io
        .block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tokio_tungstenite::connect_async_with_config(
                    url,
                    Some(
                        tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
                            .max_message_size(Some(32 * 1024 * 1024))
                            .max_frame_size(Some(32 * 1024 * 1024)),
                    ),
                    true,
                ),
            )
            .await
        })
        .map_err(|_| CodexError::Transport("VM Codex WebSocket handshake timed out.".into()))?
        .map_err(|e| CodexError::Transport(format!("VM Codex WebSocket handshake failed: {e}")))?;
    let (tx, mut rx) = mpsc::channel::<Outgoing>(128);
    let (stop, mut closing) = watch::channel(false);
    let (input_tx, mut input) = mpsc::channel::<std::io::Result<String>>(128);
    let live = access.live.clone();
    access.io.spawn(async move {
        let failure = loop {
            tokio::select! {
                _ = closing.changed() => break None,
                msg = rx.recv() => match msg {
                    Some((s, delivered)) if live.load(std::sync::atomic::Ordering::Acquire) => {
                        match ws.send(Message::Text(s.into())).await {
                            Ok(()) => { let _ = delivered.send(Ok(())); },
                            Err(error) => {
                                let message = format!("Codex WebSocket write failed: {error}");
                                let _ = delivered.send(Err(message.clone()));
                                break Some(message);
                            }
                        }
                    }
                    Some((_, delivered)) => {
                        let _ = delivered.send(Err("Codex WebSocket closed.".into()));
                        break None;
                    }
                    None => break None,
                },
                msg = ws.next() => match msg {
                    Some(Ok(Message::Text(s))) => {
                        if input_tx.send(Ok(s.to_string())).await.is_err() {
                            break None;
                        }
                    }
                    Some(Ok(Message::Ping(_))) => if ws.flush().await.is_err() {
                        break Some("Codex WebSocket closed while replying to ping.".into())
                    },
                    Some(Ok(Message::Pong(_))) => {},
                    Some(Ok(Message::Close(frame))) => {
                        break Some(format!("Codex WebSocket closed: {frame:?}"));
                    }
                    Some(Ok(_)) => break Some("Codex WebSocket received an unsupported frame.".into()),
                    Some(Err(error)) => break Some(format!("Codex WebSocket read failed: {error}")),
                    None => break Some("Codex WebSocket closed.".into()),
                },
            }
        };
        if let Some(error) = failure {
            let _ = input_tx.send(Err(std::io::Error::other(error))).await;
        }
    });
    Ok((
        Writer::WebSocket(tx, stop, access.live.clone()),
        Box::new(std::iter::from_fn(move || input.blocking_recv())),
    ))
}
