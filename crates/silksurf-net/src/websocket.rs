/*
 * websocket.rs -- blocking WebSocket probe client.
 *
 * WebSocket traffic uses Tokio only inside this module's current-thread
 * runtime. Callers keep a synchronous API, and the GUI does not own an async
 * executor just to prove long-lived browser transport support.
 */

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::NetError;

const WEBSOCKET_OPERATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WEBSOCKET_MESSAGE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebSocketReply {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

pub fn websocket_text_roundtrip(url: &str, message: &str) -> Result<WebSocketReply, NetError> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|err| NetError::new(format!("websocket runtime: {err}")))?;
    runtime.block_on(async {
        tokio::time::timeout(
            WEBSOCKET_OPERATION_TIMEOUT,
            websocket_text_roundtrip_async(url, message),
        )
        .await
        .map_err(|_| NetError::new("websocket operation timed out"))?
    })
}

async fn websocket_text_roundtrip_async(
    url: &str,
    message: &str,
) -> Result<WebSocketReply, NetError> {
    let (mut socket, _) = connect_async(url)
        .await
        .map_err(|err| NetError::new(format!("websocket connect: {err}")))?;
    socket
        .send(Message::Text(message.to_string().into()))
        .await
        .map_err(|err| NetError::new(format!("websocket send: {err}")))?;
    let reply = read_websocket_reply(&mut socket).await?;
    let _ = socket.close(None).await;
    Ok(reply)
}

async fn read_websocket_reply<S>(socket: &mut S) -> Result<WebSocketReply, NetError>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(next_message) = socket.next().await {
        let message =
            next_message.map_err(|err| NetError::new(format!("websocket receive: {err}")))?;
        match message {
            Message::Text(text) => {
                let text = text.to_string();
                enforce_message_bound(text.len())?;
                return Ok(WebSocketReply::Text(text));
            }
            Message::Binary(bytes) => {
                enforce_message_bound(bytes.len())?;
                return Ok(WebSocketReply::Binary(bytes.to_vec()));
            }
            Message::Close(_) => return Ok(WebSocketReply::Close),
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    Err(NetError::new("websocket closed before reply"))
}

fn enforce_message_bound(len: usize) -> Result<(), NetError> {
    if len > MAX_WEBSOCKET_MESSAGE_BYTES {
        return Err(NetError::new(format!(
            "websocket message exceeds {MAX_WEBSOCKET_MESSAGE_BYTES} bytes"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;

    use super::*;

    #[test]
    fn websocket_text_roundtrip_reads_echo_reply() {
        let (addr_tx, addr_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
                .expect("runtime builds");
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                    .await
                    .expect("listener binds");
                addr_tx
                    .send(listener.local_addr().expect("listener has address"))
                    .expect("address sends");
                let (stream, _) = listener.accept().await.expect("client connects");
                let mut socket = tokio_tungstenite::accept_async(stream)
                    .await
                    .expect("websocket accepts");
                if let Some(Ok(message)) = socket.next().await {
                    socket.send(message).await.expect("echo sends");
                }
                let _ = socket.close(None).await;
            });
        });
        let addr = addr_rx.recv().expect("server reports address");
        let reply = websocket_text_roundtrip(&format!("ws://{addr}/chat"), "ping")
            .expect("roundtrip succeeds");

        assert_eq!(reply, WebSocketReply::Text("ping".to_string()));
        server.join().expect("server exits");
    }

    #[test]
    fn websocket_message_bound_rejects_oversized_payload() {
        assert!(enforce_message_bound(MAX_WEBSOCKET_MESSAGE_BYTES).is_ok());
        assert!(enforce_message_bound(MAX_WEBSOCKET_MESSAGE_BYTES + 1).is_err());
    }
}
