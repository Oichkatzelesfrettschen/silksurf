/*
 * websocket_session -- persistent duplex WebSocket transport.
 *
 * One background thread per session owns a current-thread tokio runtime and
 * the tokio-tungstenite socket. Outbound frames arrive over an unbounded
 * tokio channel (sync send from the caller thread); inbound frames and
 * lifecycle transitions flow back over a std mpsc channel the caller polls
 * with try_recv from its event loop. Neither channel blocks the JS thread.
 *
 * The one-shot probe client in websocket.rs stays for its conformance tests;
 * this module is the transport a living page (chat stream, live socket)
 * rides.
 */

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const MAX_SESSION_MESSAGE_BYTES: usize = 1024 * 1024;

/// Frames the caller pushes toward the server.
#[derive(Debug)]
enum WsOutbound {
    Text(String),
    Close,
}

/// Frames and lifecycle transitions the caller drains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsIncoming {
    /// Handshake completed; the socket is open for traffic.
    Open,
    Text(String),
    Binary(Vec<u8>),
    /// The socket closed (either side); no further events follow.
    Closed,
    /// Connect or transport failure; a Closed follows immediately.
    Error(String),
}

pub struct WebSocketSession {
    outbound: UnboundedSender<WsOutbound>,
    incoming: Receiver<WsIncoming>,
    /// Kept for lifetime ownership; the thread exits when the socket closes
    /// or the outbound channel drops. Never joined on the caller thread.
    _thread: JoinHandle<()>,
}

impl WebSocketSession {
    /// Open a session toward `url`. Returns immediately; `Open` (or
    /// `Error` + `Closed`) arrives through `try_next`.
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (outbound_tx, outbound_rx) = unbounded_channel();
        let (incoming_tx, incoming_rx) = channel();
        let url = url.to_string();
        let thread = std::thread::spawn(move || {
            session_thread(&url, outbound_rx, &incoming_tx);
        });
        Self {
            outbound: outbound_tx,
            incoming: incoming_rx,
            _thread: thread,
        }
    }

    /// Queue a text frame. Frames sent before the handshake completes are
    /// delivered once the socket opens (the channel buffers them). Returns
    /// false when the session thread has exited.
    pub fn send_text(&self, text: String) -> bool {
        self.outbound.send(WsOutbound::Text(text)).is_ok()
    }

    /// Request a clean close. Idempotent; safe after the thread exited.
    pub fn close(&self) {
        let _ = self.outbound.send(WsOutbound::Close);
    }

    /// Pull the next pending event, if any. Never blocks.
    pub fn try_next(&self) -> Option<WsIncoming> {
        self.incoming.try_recv().ok()
    }
}

impl Drop for WebSocketSession {
    fn drop(&mut self) {
        let _ = self.outbound.send(WsOutbound::Close);
    }
}

fn session_thread(
    url: &str,
    mut outbound: tokio::sync::mpsc::UnboundedReceiver<WsOutbound>,
    incoming: &Sender<WsIncoming>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = incoming.send(WsIncoming::Error(format!("websocket runtime: {err}")));
            let _ = incoming.send(WsIncoming::Closed);
            return;
        }
    };
    runtime.block_on(async {
        let (mut socket, _) = match connect_async(url).await {
            Ok(pair) => pair,
            Err(err) => {
                let _ = incoming.send(WsIncoming::Error(format!("websocket connect: {err}")));
                let _ = incoming.send(WsIncoming::Closed);
                return;
            }
        };
        let _ = incoming.send(WsIncoming::Open);
        loop {
            tokio::select! {
                frame = outbound.recv() => match frame {
                    Some(WsOutbound::Text(text)) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            let _ = incoming.send(WsIncoming::Error(
                                "websocket send failed".to_string(),
                            ));
                            break;
                        }
                    }
                    // Close request or caller dropped: either way, shut down.
                    Some(WsOutbound::Close) | None => {
                        let _ = socket.close(None).await;
                        break;
                    }
                },
                message = socket.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        let text = text.to_string();
                        if text.len() > MAX_SESSION_MESSAGE_BYTES {
                            let _ = incoming.send(WsIncoming::Error(
                                "websocket message exceeds size bound".to_string(),
                            ));
                            break;
                        }
                        let _ = incoming.send(WsIncoming::Text(text));
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if bytes.len() > MAX_SESSION_MESSAGE_BYTES {
                            let _ = incoming.send(WsIncoming::Error(
                                "websocket message exceeds size bound".to_string(),
                            ));
                            break;
                        }
                        let _ = incoming.send(WsIncoming::Binary(bytes.to_vec()));
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_))) => {}
                    Some(Err(err)) => {
                        let _ = incoming.send(WsIncoming::Error(format!(
                            "websocket receive: {err}"
                        )));
                        break;
                    }
                },
            }
        }
        let _ = incoming.send(WsIncoming::Closed);
    });
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::*;

    fn wait_for(session: &WebSocketSession, max_wall: Duration) -> Option<WsIncoming> {
        let deadline = Instant::now() + max_wall;
        loop {
            if let Some(event) = session.try_next() {
                return Some(event);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn session_opens_echoes_multiple_frames_and_closes() {
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
                // Echo two frames, then push a server-initiated frame with no
                // preceding client send -- the behavior the one-shot probe
                // client could never observe.
                for _ in 0..2 {
                    if let Some(Ok(message)) = socket.next().await {
                        socket.send(message).await.expect("echo sends");
                    }
                }
                socket
                    .send(Message::Text("server-push".to_string().into()))
                    .await
                    .expect("push sends");
                let _ = socket.close(None).await;
            });
        });
        let addr = addr_rx.recv().expect("server reports address");
        let session = WebSocketSession::connect(&format!("ws://{addr}/chat"));

        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Open)
        );
        assert!(session.send_text("one".to_string()));
        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Text("one".to_string()))
        );
        assert!(session.send_text("two".to_string()));
        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Text("two".to_string()))
        );
        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Text("server-push".to_string()))
        );
        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Closed)
        );
        server.join().expect("server exits");
    }

    #[test]
    fn session_connect_failure_reports_error_then_closed() {
        let session = WebSocketSession::connect("ws://127.0.0.1:1/dead");
        let first = wait_for(&session, Duration::from_secs(10));
        assert!(matches!(first, Some(WsIncoming::Error(_))), "{first:?}");
        assert_eq!(
            wait_for(&session, Duration::from_secs(5)),
            Some(WsIncoming::Closed)
        );
    }
}
