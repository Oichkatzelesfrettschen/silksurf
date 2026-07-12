//! XMLHttpRequest shim over the blocking net path.
//!
//! The request runs synchronously inside `send()`, so the readyState
//! progression and the load event fire before `send()` returns. These tests
//! drive a minimal local HTTP echo server and assert the classic XHR surface
//! benchmark harnesses rely on: status, responseText, headers, readyState,
//! and the load / readystatechange callbacks.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use silksurf_js::SilkContext;

/// Serve exactly `request_count` HTTP/1.1 requests, echoing method and body as
/// JSON, then exit so the thread joins. Each response carries a custom header
/// so `getResponseHeader` has something non-trivial to read.
fn start_echo_server(request_count: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("echo server binds");
    let addr = listener.local_addr().expect("echo server has local addr");
    let handle = thread::spawn(move || {
        for _ in 0..request_count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            // Read headers, then the Content-Length-worth of body bytes.
            let mut header_end = None;
            while header_end.is_none() {
                let n = stream.read(&mut chunk).expect("server reads request");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                header_end = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|pos| pos + 4);
            }
            let header_end = header_end.unwrap_or(buf.len());
            let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
            let method = head.split_whitespace().next().unwrap_or("GET").to_string();
            let content_length = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while buf.len() < header_end + content_length {
                let n = stream.read(&mut chunk).expect("server reads body");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
            let payload = format!("{{\"method\":\"{method}\",\"body\":\"{body}\"}}");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Silksurf-Echo: ok\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            stream
                .write_all(response.as_bytes())
                .expect("server writes response");
        }
    });
    (format!("http://{addr}/"), handle)
}

#[test]
fn get_request_populates_status_body_and_ready_state() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "globalThis.states = []; \
         var xhr = new XMLHttpRequest(); \
         xhr.onreadystatechange = function () {{ states.push(xhr.readyState); }}; \
         xhr.onload = function () {{ globalThis.loaded = true; globalThis.body = xhr.responseText; }}; \
         xhr.open('GET', '{url}'); \
         xhr.send(); \
         globalThis.finalStatus = xhr.status; \
         globalThis.finalState = xhr.readyState;",
    ))
    .expect("xhr GET script runs");

    server.join().expect("server thread joins");

    ctx.eval(
        "if (globalThis.finalStatus !== 200) throw new Error('status ' + globalThis.finalStatus); \
         if (globalThis.finalState !== 4) throw new Error('state ' + globalThis.finalState); \
         if (globalThis.loaded !== true) throw new Error('onload did not fire'); \
         if (globalThis.body.indexOf('\"method\":\"GET\"') < 0) throw new Error('body ' + globalThis.body); \
         if (states.indexOf(1) < 0 || states.indexOf(4) < 0) throw new Error('states ' + states.join(',')); \
         if (states[states.length - 1] !== 4) throw new Error('last state ' + states.join(','));",
    )
    .expect("xhr GET assertions hold");
}

#[test]
fn post_request_sends_body_and_reads_response_headers() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "var xhr = new XMLHttpRequest(); \
         xhr.open('POST', '{url}'); \
         xhr.setRequestHeader('Content-Type', 'text/plain'); \
         xhr.send('hello'); \
         globalThis.status = xhr.status; \
         globalThis.body = xhr.responseText; \
         globalThis.echoHeader = xhr.getResponseHeader('X-Silksurf-Echo'); \
         globalThis.allHeaders = xhr.getAllResponseHeaders();",
    ))
    .expect("xhr POST script runs");

    server.join().expect("server thread joins");

    ctx.eval(
        "if (globalThis.status !== 200) throw new Error('status ' + globalThis.status); \
         if (globalThis.body.indexOf('\"method\":\"POST\"') < 0) throw new Error('method ' + globalThis.body); \
         if (globalThis.body.indexOf('\"body\":\"hello\"') < 0) throw new Error('body ' + globalThis.body); \
         if (globalThis.echoHeader !== 'ok') throw new Error('echo header ' + globalThis.echoHeader); \
         if (globalThis.allHeaders.toLowerCase().indexOf('content-type') < 0) throw new Error('headers ' + globalThis.allHeaders);",
    )
    .expect("xhr POST assertions hold");
}

#[test]
fn add_event_listener_load_fires_alongside_onload() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "globalThis.hits = 0; \
         var xhr = new XMLHttpRequest(); \
         xhr.addEventListener('load', function () {{ globalThis.hits += 1; }}); \
         xhr.addEventListener('load', function () {{ globalThis.hits += 10; }}); \
         xhr.open('GET', '{url}'); \
         xhr.send();",
    ))
    .expect("xhr listener script runs");

    server.join().expect("server thread joins");

    ctx.eval("if (globalThis.hits !== 11) throw new Error('hits ' + globalThis.hits);")
        .expect("both load listeners fire once");
}

#[test]
fn connection_failure_reports_error_and_zero_status() {
    // Port 1 refuses connections synchronously; no server needed.
    let mut ctx = SilkContext::new();
    ctx.eval(
        "globalThis.errored = false; \
         var xhr = new XMLHttpRequest(); \
         xhr.onerror = function () { globalThis.errored = true; }; \
         xhr.open('GET', 'http://127.0.0.1:1/'); \
         xhr.send(); \
         globalThis.status = xhr.status; \
         globalThis.state = xhr.readyState;",
    )
    .expect("xhr error script runs");

    ctx.eval(
        "if (globalThis.errored !== true) throw new Error('onerror did not fire'); \
         if (globalThis.status !== 0) throw new Error('status ' + globalThis.status); \
         if (globalThis.state !== 4) throw new Error('state ' + globalThis.state);",
    )
    .expect("xhr error assertions hold");
}

#[test]
fn ready_state_constants_exposed_on_constructor() {
    let mut ctx = SilkContext::new();
    ctx.eval(
        "if (XMLHttpRequest.UNSENT !== 0) throw new Error('UNSENT'); \
         if (XMLHttpRequest.OPENED !== 1) throw new Error('OPENED'); \
         if (XMLHttpRequest.HEADERS_RECEIVED !== 2) throw new Error('HEADERS_RECEIVED'); \
         if (XMLHttpRequest.LOADING !== 3) throw new Error('LOADING'); \
         if (XMLHttpRequest.DONE !== 4) throw new Error('DONE');",
    )
    .expect("readyState constants present");
}
