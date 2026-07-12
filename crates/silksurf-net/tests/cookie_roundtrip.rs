//! HTTP cookie round-trip through BasicClient with a partitioned shared jar.
//!
//! A local server sets cookies on responses and echoes back the `Cookie` header
//! it receives. With a partitioned jar and the navigation's top-level site
//! attached, the second request carries the cookie (Set-Cookie in / Cookie
//! out), cookies are isolated per top-level site, and SameSite is enforced for
//! cross-site subresources.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use silksurf_net::cookie::{PartitionedCookieStore, partition_key};
use silksurf_net::{BasicClient, HttpMethod, HttpRequest, NetClient};

/// Serve `request_count` HTTP/1.1 requests. Each response sets the cookies in
/// `set_cookie` and its body echoes the `Cookie` header the request carried.
fn start_cookie_server(
    request_count: usize,
    set_cookie: &'static [&'static str],
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("server binds");
    let addr = listener.local_addr().expect("local addr");
    let handle = thread::spawn(move || {
        for _ in 0..request_count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut chunk).expect("read request");
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
            }
            let head = String::from_utf8_lossy(&buf).to_string();
            let received = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.trim()
                        .eq_ignore_ascii_case("cookie")
                        .then(|| value.trim().to_string())
                })
                .unwrap_or_default();
            let body = format!("GOT:{received}");
            let mut response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for cookie in set_cookie {
                use std::fmt::Write as _;
                writeln!(response, "Set-Cookie: {cookie}\r").ok();
            }
            response.push_str("\r\n");
            response.push_str(&body);
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        }
    });
    (format!("http://{addr}/"), handle)
}

fn get(url: &str) -> HttpRequest {
    HttpRequest {
        method: HttpMethod::Get,
        url: url.to_string(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

#[test]
fn set_cookie_from_response_is_sent_on_the_next_request() {
    let (url, server) = start_cookie_server(2, &["sid=abc123; Path=/"]);
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    // The site is port-independent, so the top-level site is http://127.0.0.1.
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "http://127.0.0.1");

    let first = client.fetch(&get(&url)).expect("first request");
    assert_eq!(String::from_utf8_lossy(&first.body), "GOT:");

    let second = client.fetch(&get(&url)).expect("second request");
    assert_eq!(
        String::from_utf8_lossy(&second.body),
        "GOT:sid=abc123",
        "second request carried the cookie from its partition"
    );
    server.join().expect("server joins");
}

#[test]
fn cookies_are_isolated_by_top_level_site() {
    // The same resource (127.0.0.1) under two top-level sites gets two
    // partitions; a cookie set under one is invisible under the other.
    let (url, server) = start_cookie_server(2, &["tracker=1; Path=/"]);
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));

    let under_a =
        BasicClient::new().with_cookie_context(Arc::clone(&jar), "https://site-a.example");
    let under_b =
        BasicClient::new().with_cookie_context(Arc::clone(&jar), "https://site-b.example");

    under_a.fetch(&get(&url)).expect("load under A");
    let b_response = under_b.fetch(&get(&url)).expect("load under B");
    assert_eq!(
        String::from_utf8_lossy(&b_response.body),
        "GOT:",
        "site B's partition does not contain site A's cookie"
    );

    let jar = jar.lock().unwrap();
    let key_a = partition_key("https://site-a.example", "http://127.0.0.1");
    assert!(jar.store(&key_a).is_some_and(|store| !store.is_empty()));
    server.join().expect("server joins");
}

#[test]
fn empty_top_level_site_sends_cookies_unpartitioned() {
    // Degradation path: no top-level site means unpartitioned, no enforcement,
    // but cookies still round-trip (batch-11 behavior preserved).
    let (url, server) = start_cookie_server(2, &["s=1; Path=/"]);
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "");

    client.fetch(&get(&url)).expect("first");
    let second = client.fetch(&get(&url)).expect("second");
    assert_eq!(String::from_utf8_lossy(&second.body), "GOT:s=1");
    server.join().expect("server joins");
}

#[test]
fn cross_site_subresource_withholds_lax_and_strict_cookies() {
    // Strict and Lax cookies in the (127.0.0.1, third-party.example) partition,
    // requested as a cross-site subresource: neither is sent.
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    {
        let mut jar = jar.lock().unwrap();
        let key = partition_key("https://third-party.example", "http://127.0.0.1");
        let store = jar.store_mut(&key);
        store.set_from_set_cookie("strict=1; SameSite=Strict", "127.0.0.1", 0);
        store.set_from_set_cookie("lax=1; SameSite=Lax", "127.0.0.1", 0);
    }
    let (url, server) = start_cookie_server(1, &[]);
    let client =
        BasicClient::new().with_cookie_context(Arc::clone(&jar), "https://third-party.example");
    let response = client.fetch(&get(&url)).expect("cross-site subresource");
    assert_eq!(
        String::from_utf8_lossy(&response.body),
        "GOT:",
        "cross-site subresource withholds SameSite=Strict and Lax cookies"
    );
    server.join().expect("server joins");
}

#[test]
fn same_site_subresource_sends_lax_cookies() {
    // The counterpart: a same-site subresource DOES send the Lax cookie, so the
    // cross-site withholding above is enforcement, not a blanket drop.
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    {
        let mut jar = jar.lock().unwrap();
        let key = partition_key("http://127.0.0.1", "http://127.0.0.1");
        jar.store_mut(&key)
            .set_from_set_cookie("lax=1; SameSite=Lax", "127.0.0.1", 0);
    }
    let (url, server) = start_cookie_server(1, &[]);
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "http://127.0.0.1");
    let response = client.fetch(&get(&url)).expect("same-site subresource");
    assert_eq!(
        String::from_utf8_lossy(&response.body),
        "GOT:lax=1",
        "same-site subresource sends the Lax cookie"
    );
    server.join().expect("server joins");
}

/// Seed a first-party partition (top-level == resource == 127.0.0.1) with one
/// cookie of each SameSite value, so the navigation tests below observe exactly
/// which values ride a given navigation.
fn seed_first_party_same_site_cookies(jar: &Arc<Mutex<PartitionedCookieStore>>) {
    let mut jar = jar.lock().unwrap();
    let key = partition_key("http://127.0.0.1", "http://127.0.0.1");
    let store = jar.store_mut(&key);
    store.set_from_set_cookie("strict=1; SameSite=Strict", "127.0.0.1", 0);
    store.set_from_set_cookie("lax=1; SameSite=Lax", "127.0.0.1", 0);
    store.set_from_set_cookie("none=1; SameSite=None", "127.0.0.1", 0);
}

#[test]
fn same_site_top_level_navigation_sends_strict() {
    // A same-site navigation (initiator == destination) sends every cookie,
    // including Strict. This is the contrast that proves cross-site withholding
    // below is enforcement, not a blanket drop.
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    seed_first_party_same_site_cookies(&jar);
    let (url, server) = start_cookie_server(1, &[]);
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "http://127.0.0.1");
    let response = client
        .fetch_navigation(&get(&url), Some("http://127.0.0.1"))
        .expect("same-site navigation");
    let body = String::from_utf8_lossy(&response.body);
    assert!(
        body.contains("strict=1") && body.contains("lax=1") && body.contains("none=1"),
        "same-site navigation sends Strict, Lax and None: {body}"
    );
    server.join().expect("server joins");
}

#[test]
fn cross_site_top_level_get_withholds_strict_sends_lax() {
    // A cross-site link click (safe method): Strict is withheld, Lax and None
    // ride the navigation. Pre-fix this sent Strict, since the top-level site is
    // the destination and the subresource rule saw it as same-site -- the CSRF
    // exposure this feature closes.
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    seed_first_party_same_site_cookies(&jar);
    let (url, server) = start_cookie_server(1, &[]);
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "http://127.0.0.1");
    let response = client
        .fetch_navigation(&get(&url), Some("https://evil.example"))
        .expect("cross-site navigation");
    let body = String::from_utf8_lossy(&response.body);
    assert!(
        !body.contains("strict=1"),
        "cross-site top-level navigation withholds Strict: {body}"
    );
    assert!(
        body.contains("lax=1") && body.contains("none=1"),
        "cross-site top-level GET sends Lax and None: {body}"
    );
    server.join().expect("server joins");
}

#[test]
fn cross_site_top_level_post_withholds_lax() {
    // A cross-site form POST (unsafe method): Lax does not ride an unsafe
    // cross-site navigation, so only None is sent.
    let jar = Arc::new(Mutex::new(PartitionedCookieStore::new()));
    seed_first_party_same_site_cookies(&jar);
    let (url, server) = start_cookie_server(1, &[]);
    let client = BasicClient::new().with_cookie_context(Arc::clone(&jar), "http://127.0.0.1");
    let request = HttpRequest {
        method: HttpMethod::Post,
        url: url.clone(),
        headers: Vec::new(),
        body: b"field=value".to_vec(),
    };
    let response = client
        .fetch_navigation(&request, Some("https://evil.example"))
        .expect("cross-site POST navigation");
    let body = String::from_utf8_lossy(&response.body);
    assert!(
        !body.contains("strict=1") && !body.contains("lax=1"),
        "unsafe cross-site navigation withholds Strict and Lax: {body}"
    );
    assert!(
        body.contains("none=1"),
        "SameSite=None still rides an unsafe cross-site navigation: {body}"
    );
    server.join().expect("server joins");
}

#[test]
fn client_without_context_sends_no_cookies() {
    let (url, server) = start_cookie_server(1, &["a=1; Path=/"]);
    let client = BasicClient::new();
    let response = client.fetch(&get(&url)).expect("request");
    assert_eq!(String::from_utf8_lossy(&response.body), "GOT:");
    server.join().expect("server joins");
}
