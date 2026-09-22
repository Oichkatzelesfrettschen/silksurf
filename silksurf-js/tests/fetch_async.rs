//! fetch() over the host-net-completion-queue.
//!
//! The request runs on a worker thread; the promise settles only when
//! `run_host_callbacks` drains the completion queue. These tests prove the
//! promise is genuinely pending until the drain (no pre-resolution), that
//! method/body/headers from the init object reach the wire, and that
//! failures reject instead of hanging the loop.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use silksurf_js::SilkContext;

fn cookie_context(base: &str) -> SilkContext {
    let mut dom = silksurf_dom::Dom::new();
    dom.create_document();
    let dom = Arc::new(Mutex::new(dom));
    let jar = Arc::new(Mutex::new(
        silksurf_net::cookie::PartitionedCookieStore::new(),
    ));
    let url = url::Url::parse(base).expect("server URL");
    let mut context = SilkContext::with_dom_and_cookies(
        &dom,
        &jar,
        &silksurf_net::cookie::site_of_url(&url),
        "127.0.0.1",
    );
    context.set_document_url(base);
    context
        .eval("document.cookie = 'session=local; Path=/; SameSite=Lax'")
        .expect("session cookie");
    context
}

fn cookie_server(count: usize) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("cookie server binds");
    let address = listener.local_addr().expect("server address");
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        for sequence in 1..=count {
            let (mut stream, _) = listener.accept().expect("request accepted");
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .expect("read deadline");
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).expect("request header");
                assert!(count > 0, "complete request header");
                request.extend_from_slice(&buffer[..count]);
            }
            sender
                .send(String::from_utf8(request).expect("HTTP request text"))
                .expect("capture request");
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nSet-Cookie: network={sequence}; Path=/; SameSite=Lax\r\nConnection: close\r\n\r\nok").expect("response");
        }
    });
    (format!("http://{address}/app/page"), receiver, worker)
}

#[test]
fn relative_fetch_shares_cookies_and_omit_suppresses_both_directions() {
    let (base, requests, server) = cookie_server(3);
    let mut context = cookie_context(&base);
    context.eval("globalThis.done = false; globalThis.failure = '';\n\
        fetch('../api#local').then(function () {\n\
            if (!document.cookie.includes('network=1')) throw new Error('response cookie missing');\n\
            return fetch('/omit', {credentials:'omit', headers:{Cookie:'forged=1', Host:'forged.example'}});\n\
        }).then(function () {\n\
            if (!document.cookie.includes('network=1')) throw new Error('omit stored a cookie');\n\
            return fetch('/again');\n\
        }).then(function () { done = true; }).catch(function (error) { failure = String(error); done = true; });").expect("fetch script");
    assert!(pump_until(&mut context, "done", Duration::from_secs(5)));
    context
        .eval("if (failure) throw new Error(failure)")
        .expect("requests settle successfully");
    let first = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("first request");
    assert!(first.starts_with("GET /api HTTP/1.1"), "{first}");
    assert!(first.contains("session=local"), "{first}");
    let omitted = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("omitted request");
    assert!(
        !omitted.to_ascii_lowercase().contains("\r\ncookie:"),
        "{omitted}"
    );
    let again = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("final request");
    assert!(again.contains("network=1"), "{again}");
    assert!(!omitted.contains("forged.example"), "{omitted}");
    server.join().expect("server exits");
}

#[test]
fn same_origin_credentials_exclude_another_port() {
    let (target, requests, server) = cookie_server(1);
    let mut context = cookie_context("http://127.0.0.1:1/app/page");
    context
        .eval(&format!(
            "globalThis.done = false; fetch({target:?}).then(function () {{ done = true; }});"
        ))
        .expect("cross-origin fetch");
    assert!(pump_until(&mut context, "done", Duration::from_secs(5)));
    let request = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("request");
    assert!(
        !request.to_ascii_lowercase().contains("\r\ncookie:"),
        "{request}"
    );
    context.eval("if (document.cookie.includes('network=')) throw new Error('cross-origin cookie stored')").expect("response cookie isolation");
    server.join().expect("server exits");
}

#[test]
fn invalid_fetch_url_and_credentials_reject_promises() {
    let mut context = cookie_context("http://127.0.0.1:1/");
    context.eval("globalThis.rejections = 0;\n\
        fetch('file:///etc/passwd').catch(function () { rejections++; });\n\
        fetch('/', {credentials:'typo'}).catch(function () { rejections++; });\n\
        fetch('http://127.0.0.1:2/', {credentials:'include'}).catch(function () { rejections++; });").expect("invalid requests return promises");
    assert!(pump_until(
        &mut context,
        "rejections === 3",
        Duration::from_secs(1)
    ));
    assert_eq!(context.inflight_network_requests(), 0);
}

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

#[test]
fn fetch_rejects_header_injection_before_connecting() {
    let mut context = cookie_context("http://127.0.0.1:1/");
    context.eval(r"globalThis.rejections = 0;
        fetch('/', {credentials:'omit', headers:{'X-Probe':'ok\r\nCookie: forged=1'}}).catch(() => rejections++);
        fetch('/', {headers:{'X-Probe\r\nCookie':'forged=1'}}).catch(() => rejections++);
        fetch('/', {headers:{'X-Probe':'bad\u0000value'}}).catch(() => rejections++);
    ").expect("header validation returns promises");
    assert!(pump_until(
        &mut context,
        "rejections === 3",
        Duration::from_secs(1)
    ));
}

fn redirect_server(destination: String) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("redirect listener");
    let address = listener.local_addr().expect("redirect address");
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("redirect request");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read deadline");
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).expect("redirect request header");
            assert!(count > 0);
            request.extend_from_slice(&buffer[..count]);
        }
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: {destination}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").expect("redirect response");
        String::from_utf8(request).expect("request text")
    });
    (format!("http://{address}/"), worker)
}

#[test]
fn same_origin_credentials_drop_cookies_on_cross_origin_redirect() {
    let (destination, requests, destination_worker) = cookie_server(1);
    let (base, origin_worker) = redirect_server(destination);
    let mut context = cookie_context(&base);
    context
        .eval("globalThis.done = false; fetch('/redirect').then(() => done = true);")
        .expect("redirect script");
    assert!(pump_until(&mut context, "done", Duration::from_secs(5)));
    assert!(
        origin_worker
            .join()
            .expect("origin completes")
            .contains("session=local")
    );
    let request = requests
        .recv_timeout(Duration::from_secs(1))
        .expect("redirect destination request");
    assert!(!request.to_ascii_lowercase().contains("\r\ncookie:"));
    context
        .eval(
            "if (document.cookie.includes('network=')) throw new Error('redirect cookie stored');",
        )
        .expect("response cookie isolation");
    destination_worker.join().expect("destination completes");
}

#[test]
fn include_rejects_cross_origin_redirect_before_destination_connect() {
    let destination = TcpListener::bind("127.0.0.1:0").expect("destination listener");
    destination
        .set_nonblocking(true)
        .expect("nonblocking accept");
    let (base, origin_worker) = redirect_server(format!(
        "http://{}/",
        destination.local_addr().expect("destination address")
    ));
    let mut context = cookie_context(&base);
    context.eval("globalThis.rejected = false; fetch('/redirect', {credentials:'include'}).catch(() => rejected = true);").expect("redirect script");
    assert!(pump_until(&mut context, "rejected", Duration::from_secs(5)));
    assert!(
        origin_worker
            .join()
            .expect("origin completes")
            .contains("session=local")
    );
    assert_eq!(
        destination
            .accept()
            .expect_err("destination stays unconnected")
            .kind(),
        std::io::ErrorKind::WouldBlock
    );
}
