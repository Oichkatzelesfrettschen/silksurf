//! A dynamic import resolved by a real HTTP fetch inside the module loader.
//!
//! The unit coverage in module_fetch_on_demand answers from a table. This
//! drives the same path over a socket, so the blocking fetch AD-032 accepts is
//! exercised as the loader performs it: the request goes out and the import's
//! promise settles inside the `run_jobs` call that ran its job.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use silksurf_js::{ModuleFetchBudget, SilkContext};

/// Serve exactly `request_count` requests, answering each with a JS module
/// whose exported value is the path that was asked for.
fn start_module_server(request_count: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("module server binds");
    let addr = listener.local_addr().expect("module server has local addr");
    let handle = thread::spawn(move || {
        for _ in 0..request_count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let Ok(n) = stream.read(&mut chunk) else {
                    return;
                };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let path = head
                .split_whitespace()
                .nth(1)
                .unwrap_or("/unknown")
                .to_string();
            let payload = format!("export const v = '{path}';");
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/javascript\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://{addr}/"), handle)
}

/// The shape the app installs: a blocking client call, returning the body.
fn http_fetcher() -> silksurf_js::ModuleFetcher {
    Box::new(|url: &str| {
        use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.to_string(),
            headers: vec![(
                "Accept".to_string(),
                "text/javascript,application/javascript,*/*".to_string(),
            )],
            body: Vec::new(),
        };
        let response = BasicClient::new()
            .fetch(&request)
            .map_err(|err| format!("{err:?}"))?;
        if response.status != 200 {
            return Err(format!("status {}", response.status));
        }
        Ok(String::from_utf8_lossy(&response.body).to_string())
    })
}

#[test]
fn a_computed_specifier_resolves_over_the_network() {
    let (base, server) = start_module_server(1);
    let mut ctx = SilkContext::new();
    ctx.set_module_fetcher(http_fetcher());
    ctx.set_module_fetch_budget(ModuleFetchBudget {
        urls: 4,
        bytes: 64 * 1024,
    });

    let root = format!("{base}root.js");
    // The specifier is built at run time, which is the case no scan over the
    // source text resolves and the static walk cannot have prefetched.
    let source = "globalThis.seen = 'pending';\
         var name = ['route', 'chunk'].join('-') + '.js';\
         import('./' + name).then(function (m) { globalThis.seen = 'ok:' + m.v; },\
         function (e) { globalThis.seen = String(e); });"
        .to_string();
    ctx.eval_module_graph(&root, &[(root.clone(), source)])
        .expect("module graph evaluates");
    ctx.run_pending_jobs();

    let Err(seen) = ctx.eval("throw new Error(String(globalThis.seen));") else {
        panic!("probe throw did not surface");
    };
    assert!(
        seen.contains("ok:/route-chunk.js"),
        "the fetched module's export reaches the importer: {seen}"
    );
    assert_eq!(ctx.module_fetch_budget().urls, 3, "the fetch charged once");
    server.join().expect("module server thread joins");
}

#[test]
fn an_import_issued_from_a_timer_callback_resolves_during_the_tick() {
    let (base, server) = start_module_server(1);
    let mut ctx = SilkContext::new();
    ctx.set_document_url(&format!("{base}index.html"));
    ctx.set_module_fetcher(http_fetcher());
    ctx.set_module_fetch_budget(ModuleFetchBudget {
        urls: 4,
        bytes: 64 * 1024,
    });

    // A route change after first paint issues its import from a callback the
    // tick drains, rather than from the module graph a page build evaluates,
    // so this is the path that blocks the thread presenting frames.
    ctx.eval(
        "globalThis.seen = 'pending';\
         setTimeout(function () {\
           import('./late-route.js').then(function (m) { globalThis.seen = 'ok:' + m.v; },\
           function (e) { globalThis.seen = String(e); });\
         }, 0);",
    )
    .expect("script evaluates");

    let ran = ctx.run_host_callbacks(16).expect("tick drains the timer");
    assert_eq!(ran, 1, "the timer callback fires on the tick");
    ctx.run_pending_jobs();

    let Err(seen) = ctx.eval("throw new Error(String(globalThis.seen));") else {
        panic!("probe throw did not surface");
    };
    assert!(
        seen.contains("ok:/late-route.js"),
        "the import settles within the tick that ran its callback: {seen}"
    );
    server.join().expect("module server thread joins");
}
