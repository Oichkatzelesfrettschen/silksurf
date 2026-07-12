//! fetch() over the host-net-completion-queue.
//!
//! The request runs on a worker thread; the promise settles only when
//! `run_host_callbacks` drains the completion queue. These tests prove the
//! promise is genuinely pending until the drain (no pre-resolution), that
//! method/body/headers from the init object reach the wire, and that
//! failures reject instead of hanging the loop.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use silksurf_js::SilkContext;

/// Serve exactly `request_count` HTTP/1.1 requests, echoing method and body
/// as JSON. Mirror of the XHR test harness.
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
            let has_probe_header = head
                .lines()
                .any(|line| line.to_ascii_lowercase().starts_with("x-silksurf-probe:"));
            let payload = format!(
                "{{\"method\":\"{method}\",\"body\":\"{body}\",\"probe\":{has_probe_header}}}"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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

/// Pump host callbacks until the predicate global becomes truthy or the
/// deadline passes. Returns whether the predicate fired.
fn pump_until(ctx: &mut SilkContext, predicate: &str, max_wall: Duration) -> bool {
    let deadline = Instant::now() + max_wall;
    loop {
        ctx.run_pending_jobs();
        let _ = ctx.run_ready_host_callbacks();
        let mut done = false;
        ctx.eval(&format!(
            "globalThis.__pumpProbe = !!({predicate});
             if (globalThis.__pumpProbe) {{ globalThis.__pumpHit = true; }}"
        ))
        .expect("predicate eval succeeds");
        ctx.eval("if (globalThis.__pumpHit) { throw new Error('HIT'); }")
            .err()
            .inspect(|message| {
                if message.contains("HIT") {
                    done = true;
                }
            });
        if done {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn fetch_promise_settles_only_via_host_callback_drain() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "globalThis.state = 'pending';
         fetch('{url}').then(function (res) {{
           globalThis.state = 'resolved:' + res.status;
         }});"
    ))
    .expect("fetch call succeeds");
    // The promise must NOT settle synchronously: the queue drains on ticks.
    ctx.run_pending_jobs();
    ctx.eval("if (globalThis.state !== 'pending') { throw new Error('settled early: ' + globalThis.state); }")
        .expect("promise still pending before host tick");
    assert!(
        ctx.has_pending_host_callbacks(),
        "in-flight request must count as pending host work"
    );
    assert!(
        pump_until(
            &mut ctx,
            "globalThis.state === 'resolved:200'",
            Duration::from_secs(5)
        ),
        "fetch promise resolves through the drain"
    );
    server.join().expect("server thread joins");
}

#[test]
fn fetch_init_method_body_and_headers_reach_the_wire() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "globalThis.echo = null;
         fetch('{url}', {{
           method: 'POST',
           body: 'payload-bytes',
           headers: {{ 'X-Silksurf-Probe': 'yes' }}
         }}).then(function (res) {{ return res.json(); }})
           .then(function (data) {{ globalThis.echo = data; }});"
    ))
    .expect("fetch call succeeds");
    assert!(
        pump_until(
            &mut ctx,
            "globalThis.echo !== null \
             && globalThis.echo.method === 'POST' \
             && globalThis.echo.body === 'payload-bytes' \
             && globalThis.echo.probe === true",
            Duration::from_secs(5)
        ),
        "POST body and custom header echo back"
    );
    server.join().expect("server thread joins");
}

#[test]
fn fetch_failure_rejects_and_clears_pending_work() {
    let mut ctx = SilkContext::new();
    // Reserved TEST-NET-1 address: connection fails fast or times out inside
    // the client; either way the completion must arrive as a rejection.
    ctx.eval(
        "globalThis.failure = null;
         fetch('http://127.0.0.1:1/unreachable').then(
           function () { globalThis.failure = 'resolved'; },
           function (err) { globalThis.failure = 'rejected'; }
         );",
    )
    .expect("fetch call succeeds");
    assert!(
        pump_until(
            &mut ctx,
            "globalThis.failure === 'rejected'",
            Duration::from_secs(10)
        ),
        "failed fetch rejects"
    );
    assert!(
        !ctx.has_pending_host_callbacks(),
        "no pending work after the rejection drains"
    );
}

#[test]
fn response_body_reader_yields_chunks_then_done() {
    let (url, server) = start_echo_server(1);
    let mut ctx = SilkContext::new();
    ctx.eval(&format!(
        "globalThis.reads = [];
         globalThis.finished = false;
         fetch('{url}').then(function (res) {{
           var reader = res.body.getReader();
           function step() {{
             return reader.read().then(function (r) {{
               if (r.done) {{ globalThis.finished = true; return; }}
               globalThis.reads.push(r.value.length);
               return step();
             }});
           }}
           return step();
         }});"
    ))
    .expect("fetch call succeeds");
    assert!(
        pump_until(
            &mut ctx,
            "globalThis.finished === true && globalThis.reads.length >= 1 && globalThis.reads[0] > 0",
            Duration::from_secs(5)
        ),
        "reader yields at least one chunk then done"
    );
    server.join().expect("server thread joins");
}
