use super::super::{transport::write_message, websocket};
use crate::remote::native::Access;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let address: SocketAddr = listener.local_addr().unwrap();
    (listener, format!("ws://{address}"))
}

async fn open(
    url: String,
) -> (
    super::super::transport::Writer,
    super::super::transport::Messages,
) {
    let access = Access::new("unused".into(), "localhost".into(), "/vm".into());
    tokio::task::spawn_blocking(move || websocket::open(&access, &url))
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn websocket_reader_backpressures_bursts_and_accepts_an_18_mib_frame() {
    let (listener, url) = listener().await;
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        for index in 0..700 {
            socket
                .send(Message::Text(index.to_string().into()))
                .await
                .unwrap();
        }
        socket
            .send(Message::Text("x".repeat(18 * 1024 * 1024).into()))
            .await
            .unwrap();
        socket.send(Message::Close(None)).await.unwrap();
    });
    let (_writer, mut messages) = open(url).await;
    let result = tokio::task::spawn_blocking(move || {
        for index in 0..700 {
            assert_eq!(messages.next().unwrap().unwrap(), index.to_string());
        }
        let frame = messages.next().unwrap().unwrap();
        let size = frame.len();
        let failure = messages.next().unwrap().unwrap_err().to_string();
        (size, failure)
    })
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(result.0, 18 * 1024 * 1024);
    assert!(result.1.contains("closed"));
}

#[tokio::test]
async fn websocket_writes_wait_until_the_socket_has_sent_the_message() {
    let (listener, url) = listener().await;
    let (received, request) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        let message = socket.next().await.unwrap().unwrap().into_text().unwrap();
        let _ = received.send(message);
        socket.send(Message::Close(None)).await.unwrap();
    });
    let (writer, _messages) = open(url).await;
    let sent = tokio::task::spawn_blocking(move || {
        write_message(&writer, &json!({"method":"turn/interrupt","params":{}}))
    })
    .await
    .unwrap();
    assert!(sent.is_ok());
    let received = tokio::time::timeout(Duration::from_secs(2), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&received).unwrap()["method"],
        "turn/interrupt"
    );
    server.await.unwrap();
}
