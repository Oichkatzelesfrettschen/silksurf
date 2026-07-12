/*
 * sse -- Server-Sent Events field parser and streaming subscription.
 *
 * The parser is a pure function over the text/event-stream wire format
 * (WHATWG HTML "Server-sent events" section 9.2): lines of `field: value`
 * accumulate into an event; a blank line dispatches it. `data:` lines join
 * with newlines, `event:` overrides the type, `id:` sets the last-event-id,
 * `retry:` carries the reconnect hint, and comment lines (leading ':') are
 * dropped.
 *
 * SseSubscription mirrors WebSocketSession's shape: a background thread owns
 * the blocking GET and pushes parsed events over a std mpsc channel the
 * caller drains with try_next from its event loop.
 */

use std::io::Read;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

/// One parsed server-sent event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Event type; "message" when the stream did not name one.
    pub event_type: String,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
}

/// Incremental parser state: feed bytes, collect dispatched events.
#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
    event_type: String,
    data_lines: Vec<String>,
    id: Option<String>,
    retry_ms: Option<u64>,
}

impl SseParser {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of stream text; returns events completed by this chunk.
    pub fn feed(&mut self, chunk: &str) -> Vec<SseEvent> {
        self.buffer.push_str(chunk);
        let mut events = Vec::new();
        while let Some(newline) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=newline).collect();
            let line = line.trim_end_matches('\n').trim_end_matches('\r');
            if line.is_empty() {
                if let Some(event) = self.dispatch() {
                    events.push(event);
                }
                continue;
            }
            if line.starts_with(':') {
                continue;
            }
            let (field, value) = match line.split_once(':') {
                Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
                None => (line, ""),
            };
            match field {
                "data" => self.data_lines.push(value.to_string()),
                "event" => self.event_type = value.to_string(),
                "id" if !value.contains('\0') => self.id = Some(value.to_string()),
                "retry" => {
                    if let Ok(ms) = value.parse::<u64>() {
                        self.retry_ms = Some(ms);
                    }
                }
                _ => {}
            }
        }
        events
    }

    /// Blank line: dispatch the accumulated event, if it carries data.
    fn dispatch(&mut self) -> Option<SseEvent> {
        let data_lines = std::mem::take(&mut self.data_lines);
        let event_type = std::mem::take(&mut self.event_type);
        if data_lines.is_empty() {
            return None;
        }
        Some(SseEvent {
            event_type: if event_type.is_empty() {
                "message".to_string()
            } else {
                event_type
            },
            data: data_lines.join("\n"),
            id: self.id.clone(),
            retry_ms: self.retry_ms,
        })
    }
}

/// Lifecycle events an EventSource consumer drains.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseIncoming {
    Open,
    Event(SseEvent),
    Error(String),
    Closed,
}

pub struct SseSubscription {
    incoming: Receiver<SseIncoming>,
    /// Kept for lifetime ownership; the thread exits when the stream ends.
    _thread: JoinHandle<()>,
}

impl SseSubscription {
    /// Open a text/event-stream subscription toward `url`. Returns
    /// immediately; Open (or Error + Closed) arrives through `try_next`.
    /// No automatic reconnect: the consumer decides using `retry_ms`.
    #[must_use]
    pub fn connect(url: &str) -> Self {
        let (tx, rx) = channel();
        let url = url.to_string();
        let thread = std::thread::spawn(move || {
            subscription_thread(&url, &tx);
        });
        Self {
            incoming: rx,
            _thread: thread,
        }
    }

    /// Pull the next pending event, if any. Never blocks.
    pub fn try_next(&self) -> Option<SseIncoming> {
        self.incoming.try_recv().ok()
    }
}

/// Blocking GET with incremental body reads. The plain TCP read loop keeps
/// bytes flowing to the parser as they arrive instead of buffering the whole
/// (unbounded) stream, which is the entire point of SSE.
fn subscription_thread(url: &str, tx: &Sender<SseIncoming>) {
    let Ok(parsed) = url::Url::parse(url) else {
        let _ = tx.send(SseIncoming::Error(format!("sse: invalid url {url}")));
        let _ = tx.send(SseIncoming::Closed);
        return;
    };
    if parsed.scheme() != "http" {
        let _ = tx.send(SseIncoming::Error(
            "sse: only http:// streams are supported (https via the h2 path is a follow-up)"
                .to_string(),
        ));
        let _ = tx.send(SseIncoming::Closed);
        return;
    }
    let host = parsed.host_str().unwrap_or_default().to_string();
    let port = parsed.port_or_known_default().unwrap_or(80);
    let path = if parsed.path().is_empty() {
        "/"
    } else {
        parsed.path()
    };

    let stream = std::net::TcpStream::connect((host.as_str(), port));
    let mut stream = match stream {
        Ok(stream) => stream,
        Err(err) => {
            let _ = tx.send(SseIncoming::Error(format!("sse connect: {err}")));
            let _ = tx.send(SseIncoming::Closed);
            return;
        }
    };
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: text/event-stream\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n"
    );
    if let Err(err) = std::io::Write::write_all(&mut stream, request.as_bytes()) {
        let _ = tx.send(SseIncoming::Error(format!("sse request: {err}")));
        let _ = tx.send(SseIncoming::Closed);
        return;
    }

    // Skip the response head, then stream the body through the parser.
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(0) => {
                let _ = tx.send(SseIncoming::Error(
                    "sse: stream ended in headers".to_string(),
                ));
                let _ = tx.send(SseIncoming::Closed);
                return;
            }
            Ok(_) => head.push(byte[0]),
            Err(err) => {
                let _ = tx.send(SseIncoming::Error(format!("sse headers: {err}")));
                let _ = tx.send(SseIncoming::Closed);
                return;
            }
        }
    }
    let head_text = String::from_utf8_lossy(&head);
    if !head_text.starts_with("HTTP/1.1 200") && !head_text.starts_with("HTTP/1.0 200") {
        let status = head_text.lines().next().unwrap_or("").to_string();
        let _ = tx.send(SseIncoming::Error(format!("sse status: {status}")));
        let _ = tx.send(SseIncoming::Closed);
        return;
    }
    let _ = tx.send(SseIncoming::Open);

    let mut parser = SseParser::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let text = String::from_utf8_lossy(&chunk[..n]).to_string();
                for event in parser.feed(&text) {
                    if tx.send(SseIncoming::Event(event)).is_err() {
                        return;
                    }
                }
            }
            Err(err) => {
                let _ = tx.send(SseIncoming::Error(format!("sse read: {err}")));
                break;
            }
        }
    }
    let _ = tx.send(SseIncoming::Closed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_dispatches_on_blank_line_with_defaults() {
        let mut parser = SseParser::new();
        let events = parser.feed("data: hello\n\n");
        assert_eq!(
            events,
            vec![SseEvent {
                event_type: "message".to_string(),
                data: "hello".to_string(),
                id: None,
                retry_ms: None,
            }]
        );
    }

    #[test]
    fn parser_joins_multiline_data_and_honors_fields() {
        let mut parser = SseParser::new();
        let events =
            parser.feed("event: delta\nid: 42\nretry: 1500\ndata: a\ndata: b\n\n: comment\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "delta");
        assert_eq!(events[0].data, "a\nb");
        assert_eq!(events[0].id.as_deref(), Some("42"));
        assert_eq!(events[0].retry_ms, Some(1500));
    }

    #[test]
    fn parser_survives_chunk_splits_mid_line() {
        let mut parser = SseParser::new();
        assert!(parser.feed("da").is_empty());
        assert!(parser.feed("ta: spl").is_empty());
        let events = parser.feed("it\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "split");
    }

    #[test]
    fn parser_carries_last_event_id_forward() {
        let mut parser = SseParser::new();
        let first = parser.feed("id: 7\ndata: one\n\n");
        let second = parser.feed("data: two\n\n");
        assert_eq!(first[0].id.as_deref(), Some("7"));
        assert_eq!(second[0].id.as_deref(), Some("7"));
    }

    #[test]
    fn subscription_streams_events_from_local_server() {
        use std::io::Write;
        use std::net::TcpListener;
        use std::time::{Duration, Instant};

        let listener = TcpListener::bind("127.0.0.1:0").expect("sse server binds");
        let addr = listener.local_addr().expect("sse server has addr");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("client connects");
            let mut discard = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut discard);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n\
                      data: first\n\nevent: delta\ndata: second\n\n",
                )
                .expect("server writes stream");
        });

        let subscription = SseSubscription::connect(&format!("http://{addr}/stream"));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut received = Vec::new();
        while Instant::now() < deadline {
            if let Some(incoming) = subscription.try_next() {
                let closed = matches!(incoming, SseIncoming::Closed);
                received.push(incoming);
                if closed {
                    break;
                }
            } else {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        server.join().expect("server exits");
        assert_eq!(received.first(), Some(&SseIncoming::Open));
        assert!(received.iter().any(|incoming| matches!(
            incoming,
            SseIncoming::Event(event) if event.data == "first" && event.event_type == "message"
        )));
        assert!(received.iter().any(|incoming| matches!(
            incoming,
            SseIncoming::Event(event) if event.data == "second" && event.event_type == "delta"
        )));
        assert_eq!(received.last(), Some(&SseIncoming::Closed));
    }
}
