/*
 * net/lib.rs -- pure Rust HTTP client (HTTP/1.1 + HTTP/2).
 *
 * WHY: SilkSurf needs to fetch web pages, CSS, and JS resources. This
 * client uses only synchronous std::net::TcpStream + rustls for TLS,
 * with httparse for zero-copy header parsing. No tokio/hyper/reqwest --
 * minimal dependency footprint and zero async runtime overhead.
 *
 * HTTP/1.1 path (BasicClient::fetch):
 *   1. Parse URL via url crate (WHATWG compliant)
 *   2. TCP connect to host:port
 *   3. TLS handshake via rustls (HTTPS only)
 *   4. Send HTTP/1.1 request with headers
 *   5. Read response (16MB limit, 30s timeout)
 *   6. Parse headers via httparse (zero-copy)
 *   7. Follow redirects (301/302/303/307/308, max 5)
 *
 * HTTP/2 path (BasicClient::fetch_parallel):
 *   - Groups same-HTTPS-host requests and tries h2 via ALPN
 *   - On h2 success: all requests multiplexed over one TLS connection
 *   - On h2 failure: falls back to sequential HTTP/1.1 per request
 *   - Internal tokio current_thread runtime (no extra OS threads)
 *
 * TLS: system certs (rustls-native-certs) + Mozilla bundle (webpki-roots).
 * --insecure flag available for environments with broken cert chains.
 *
 * DONE(perf): Response caching (Phase 4.3) -- see silksurf-engine/src/speculative.rs
 * DONE(perf): HTTP/2 parallel fetch (Phase D) -- see h2_client.rs
 *
 * See: silksurf-tls for TLS configuration and cert loading
 * See: h2_client.rs for HTTP/2 multiplexed fetch implementation
 * See: builtins/fetch_builtin.rs for JS fetch() API binding
 * See: silksurf-app/src/main.rs for webview usage
 */
#![allow(clippy::collapsible_if)]

pub mod cache;
pub mod cookie;
pub mod h2_client;
pub mod websocket;

pub use websocket::{WebSocketReply, websocket_text_roundtrip};

use rustls::StreamOwned;
use silksurf_tls::{RustlsProvider, TlsProvider};
#[cfg(feature = "content-encoding")]
use std::io::Cursor;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

/*
 * MAX_RESPONSE_BODY_BYTES -- DoS bound on a single HTTP response body.
 *
 * WHY: Without a hard cap, a malicious or misconfigured server can
 * stream an unbounded body and OOM the renderer. The original code
 * hard-coded 16 MiB inline in read_response(); promoting it to a public
 * constant lets embedders raise/lower the bound, lets tests reference
 * it explicitly, and lets the documentation site list the active value.
 *
 * Default 16 MiB. Page weight at the 95th percentile in HTTP Archive's
 * 2026 corpus is ~12 MiB across all subresources, but a single HTML
 * document or stylesheet is typically <1 MiB. 16 MiB covers very large
 * Wikipedia talk pages and a few pathological CSS bundles while still
 * fitting comfortably in a renderer's address space.
 *
 * Enforced inside read_response(): when the in-flight response Vec<u8>
 * grows past this value the read returns NetError, the connection is
 * dropped, and the caller sees a recoverable failure instead of OOM.
 *
 * See: SNAZZY-WAFFLE roadmap P8.S8 (DoS bounds per crate).
 */
pub const MAX_RESPONSE_BODY_BYTES: usize = 16 * 1024 * 1024;

#[cfg(feature = "content-encoding")]
const ACCEPT_ENCODING_VALUE: &str = "br, gzip, deflate";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    fn as_str(self) -> &'static str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Delete => "DELETE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Get a header value by name (case-insensitive).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetError {
    pub message: String,
}

impl From<NetError> for silksurf_core::SilkError {
    fn from(e: NetError) -> Self {
        silksurf_core::SilkError::Net(e.message)
    }
}

impl NetError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

pub trait NetClient {
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, NetError>;
}

/// A shared partitioned cookie jar plus the top-level site of the current
/// navigation. The site drives the per-request partition key and SameSite
/// enforcement; an empty site degrades to the unpartitioned store with no
/// enforcement (the graceful fallback when a fetch path lacks the top-level
/// site). The `Arc<Mutex<PartitionedCookieStore>>` is also shared with the JS
/// `document.cookie` bridge so cookies round-trip between HTTP and script.
#[derive(Clone)]
pub struct CookieContext {
    pub jar: Arc<Mutex<cookie::PartitionedCookieStore>>,
    pub top_level_site: String,
}

pub struct BasicClient {
    tls: Arc<dyn TlsProvider + Send + Sync>,
    max_redirects: usize,
    cookie_context: Option<CookieContext>,
}

impl BasicClient {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tls: shared_default_tls_provider(),
            max_redirects: 5,
            cookie_context: None,
        }
    }

    #[must_use]
    pub fn with_tls(tls: Arc<dyn TlsProvider + Send + Sync>) -> Self {
        Self {
            tls,
            max_redirects: 5,
            cookie_context: None,
        }
    }

    /// Attach a partitioned cookie jar and the navigation's top-level site,
    /// enabling the HTTP cookie round-trip with partitioning and SameSite
    /// enforcement. An empty `top_level_site` sends cookies unpartitioned and
    /// unenforced (batch-11 behavior) rather than dropping them.
    #[must_use]
    pub fn with_cookie_context(
        mut self,
        jar: Arc<Mutex<cookie::PartitionedCookieStore>>,
        top_level_site: impl Into<String>,
    ) -> Self {
        self.cookie_context = Some(CookieContext {
            jar,
            top_level_site: top_level_site.into(),
        });
        self
    }

    /// The TLS provider backing this client, so a caller can rebuild the client
    /// with the same TLS configuration plus a cookie context.
    #[must_use]
    pub fn tls_provider(&self) -> Arc<dyn TlsProvider + Send + Sync> {
        Arc::clone(&self.tls)
    }

    /// Compute the `Cookie` request header for a target from the partition the
    /// request belongs to (keyed by top-level site + resource site), or `None`
    /// when no context is attached, the partition is empty, or the caller
    /// already set a `Cookie` header (an explicit header wins).
    ///
    /// `nav_context` is `Some` for a top-level navigation (the SameSite posture
    /// classified from its initiator) and `None` for a subresource, where the
    /// posture is derived from the destination-vs-top-level-site comparison. An
    /// empty top-level site is not enforced.
    fn request_cookie_header(
        &self,
        request: &HttpRequest,
        target: &RequestTarget,
        nav_context: Option<cookie::SameSiteContext>,
    ) -> Option<String> {
        if has_header(&request.headers, "cookie") {
            return None;
        }
        let context = self.cookie_context.as_ref()?;
        let resource_site = cookie::site_of_url(&target.parsed);
        let partition = cookie::partition_key(&context.top_level_site, &resource_site);
        let same_site = nav_context.unwrap_or_else(|| {
            cookie::subresource_same_site_context(&context.top_level_site, &resource_site)
        });
        let jar = context.jar.lock().unwrap_or_else(PoisonError::into_inner);
        let header = jar.store(&partition).map_or_else(String::new, |store| {
            store.cookie_header(
                &target.host,
                target.parsed.path(),
                target.is_https,
                true,
                same_site,
                cookie::now_unix(),
            )
        });
        (!header.is_empty()).then_some(header)
    }

    /// Store every `Set-Cookie` header from a response into the request's
    /// partition.
    fn store_response_cookies(&self, response: &HttpResponse, target: &RequestTarget) {
        let Some(context) = self.cookie_context.as_ref() else {
            return;
        };
        let resource_site = cookie::site_of_url(&target.parsed);
        let partition = cookie::partition_key(&context.top_level_site, &resource_site);
        let mut jar = context.jar.lock().unwrap_or_else(PoisonError::into_inner);
        let store = jar.store_mut(&partition);
        let now = cookie::now_unix();
        for (name, value) in &response.headers {
            if name.eq_ignore_ascii_case("set-cookie") {
                store.set_from_set_cookie(value, &target.host, now);
            }
        }
    }
}

fn shared_default_tls_provider() -> Arc<dyn TlsProvider + Send + Sync> {
    static DEFAULT_TLS: OnceLock<Arc<RustlsProvider>> = OnceLock::new();
    let provider = DEFAULT_TLS.get_or_init(|| Arc::new(RustlsProvider::new()));
    provider.clone()
}

impl Default for BasicClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetClient for BasicClient {
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        // No nav_context: cookies are classified as a subresource (destination
        // vs top-level site). Top-level navigations use `fetch_navigation`.
        self.fetch_with_context(request, None)
    }
}

impl BasicClient {
    /// Fetch a top-level navigation, enforcing SameSite from the initiator site.
    ///
    /// `initiator_site` is the site that initiated the navigation (`None` for a
    /// browser-initiated one -- address bar, bookmark, history, initial load).
    /// The SameSite posture is computed once from the initiator, the
    /// destination site, and whether the method is safe, then applied across the
    /// redirect chain (redirect-hop reclassification is out of scope: a
    /// cross-site redirect reached from a same-site navigation is not
    /// re-flagged).
    pub fn fetch_navigation(
        &self,
        request: &HttpRequest,
        initiator_site: Option<&str>,
    ) -> Result<HttpResponse, NetError> {
        let destination_site = url::Url::parse(&request.url)
            .as_ref()
            .map(cookie::site_of_url)
            .unwrap_or_default();
        let nav_context = cookie::navigation_same_site_context(
            initiator_site,
            &destination_site,
            cookie::is_safe_method(request.method.as_str()),
        );
        self.fetch_with_context(request, Some(nav_context))
    }

    fn fetch_with_context(
        &self,
        request: &HttpRequest,
        nav_context: Option<cookie::SameSiteContext>,
    ) -> Result<HttpResponse, NetError> {
        let mut current_url = request.url.clone();
        let mut redirects = 0;

        loop {
            let (parsed, response) = self.fetch_http1_once(request, &current_url, nav_context)?;
            if let Some(next_url) =
                redirect_target(&parsed, &response, redirects, self.max_redirects)
            {
                current_url = next_url;
                redirects += 1;
                continue;
            }
            return Ok(response);
        }
    }

    fn fetch_http1_once(
        &self,
        request: &HttpRequest,
        current_url: &str,
        nav_context: Option<cookie::SameSiteContext>,
    ) -> Result<(url::Url, HttpResponse), NetError> {
        let target = RequestTarget::parse(current_url)?;
        let cookie_header = self.request_cookie_header(request, &target, nav_context);
        let request_bytes = build_http1_request(request, &target, cookie_header.as_deref())?;
        let response_bytes = send_http1_request(self.tls.as_ref(), &target, &request_bytes)?;
        let response = parse_response(&response_bytes)?;
        self.store_response_cookies(&response, &target);
        Ok((target.parsed, response))
    }
}

struct RequestTarget {
    parsed: url::Url,
    host: String,
    is_https: bool,
    port: u16,
    path: String,
}

impl RequestTarget {
    fn parse(current_url: &str) -> Result<Self, NetError> {
        let parsed =
            url::Url::parse(current_url).map_err(|e| NetError::new(format!("Invalid URL: {e}")))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| NetError::new("No host in URL"))?
            .to_string();
        let is_https = parsed.scheme() == "https";
        let port = parsed.port().unwrap_or(if is_https { 443 } else { 80 });
        let path = request_path(&parsed);
        Ok(Self {
            parsed,
            host,
            is_https,
            port,
            path,
        })
    }
}

fn request_path(parsed: &url::Url) -> String {
    match parsed.query() {
        Some(query) => format!("{}?{query}", parsed.path()),
        None => parsed.path().to_string(),
    }
}

fn build_http1_request(
    request: &HttpRequest,
    target: &RequestTarget,
    cookie_header: Option<&str>,
) -> Result<Vec<u8>, NetError> {
    let mut request_bytes = Vec::with_capacity(512);
    write_request_line(&mut request_bytes, request, target)?;
    write_content_encoding_header(&mut request_bytes, &request.headers)?;
    write_custom_headers(&mut request_bytes, &request.headers)?;
    if let Some(cookie_header) = cookie_header {
        write!(request_bytes, "Cookie: {cookie_header}\r\n")
            .map_err(|e| NetError::new(format!("Write error: {e}")))?;
    }
    write_content_length(&mut request_bytes, request.body.len())?;
    request_bytes.extend_from_slice(b"\r\n");
    request_bytes.extend_from_slice(&request.body);
    Ok(request_bytes)
}

fn write_request_line(
    request_bytes: &mut Vec<u8>,
    request: &HttpRequest,
    target: &RequestTarget,
) -> Result<(), NetError> {
    let path = if target.path.is_empty() {
        "/"
    } else {
        target.path.as_str()
    };
    write!(
        request_bytes,
        "{} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: SilkSurf/0.1\r\nAccept: */*\r\n",
        request.method.as_str(),
        target.host,
    )
    .map_err(|e| NetError::new(format!("Write error: {e}")))
}

#[cfg(feature = "content-encoding")]
fn write_content_encoding_header(
    request_bytes: &mut Vec<u8>,
    headers: &[(String, String)],
) -> Result<(), NetError> {
    if !has_header(headers, "accept-encoding") {
        write!(
            request_bytes,
            "Accept-Encoding: {ACCEPT_ENCODING_VALUE}\r\n"
        )
        .map_err(|e| NetError::new(format!("Write error: {e}")))?;
    }
    Ok(())
}

#[cfg(not(feature = "content-encoding"))]
fn write_content_encoding_header(
    _request_bytes: &mut Vec<u8>,
    _headers: &[(String, String)],
) -> Result<(), NetError> {
    Ok(())
}

fn write_custom_headers(
    request_bytes: &mut Vec<u8>,
    headers: &[(String, String)],
) -> Result<(), NetError> {
    for (name, value) in headers {
        write!(request_bytes, "{name}: {value}\r\n")
            .map_err(|e| NetError::new(format!("Write error: {e}")))?;
    }
    Ok(())
}

fn write_content_length(request_bytes: &mut Vec<u8>, body_len: usize) -> Result<(), NetError> {
    if body_len == 0 {
        return Ok(());
    }
    write!(request_bytes, "Content-Length: {body_len}\r\n")
        .map_err(|e| NetError::new(format!("Write error: {e}")))
}

fn send_http1_request(
    tls: &(dyn TlsProvider + Send + Sync),
    target: &RequestTarget,
    request_bytes: &[u8],
) -> Result<Vec<u8>, NetError> {
    let mut tcp = connect_tcp(target)?;
    if target.is_https {
        return send_https_request(tls, target, tcp, request_bytes);
    }
    tcp.write_all(request_bytes)
        .map_err(|e| NetError::new(format!("TCP write: {e}")))?;
    read_response(&mut tcp)
}

fn connect_tcp(target: &RequestTarget) -> Result<TcpStream, NetError> {
    let addr = format!("{}:{}", target.host, target.port);
    let tcp = TcpStream::connect(&addr)
        .map_err(|e| NetError::new(format!("TCP connect to {addr}: {e}")))?;
    tcp.set_read_timeout(Some(std::time::Duration::from_secs(
        silksurf_tls::MAX_TLS_HANDSHAKE_SECS,
    )))
    .ok();
    Ok(tcp)
}

fn send_https_request(
    tls: &(dyn TlsProvider + Send + Sync),
    target: &RequestTarget,
    tcp: TcpStream,
    request_bytes: &[u8],
) -> Result<Vec<u8>, NetError> {
    let server_name = rustls::pki_types::ServerName::try_from(target.host.as_str())
        .map_err(|e| NetError::new(format!("Invalid server name: {e}")))?
        .to_owned();
    let config = tls.config();
    let conn = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| NetError::new(format!("TLS setup: {e}")))?;
    complete_tls_handshake(conn, tcp, request_bytes)
}

fn complete_tls_handshake(
    mut conn: rustls::ClientConnection,
    mut tcp: TcpStream,
    request_bytes: &[u8],
) -> Result<Vec<u8>, NetError> {
    while conn.is_handshaking() {
        conn.complete_io(&mut tcp)
            .map_err(|e| NetError::new(format!("TLS handshake: {e}")))?;
    }
    let mut stream = StreamOwned::new(conn, tcp);
    stream
        .write_all(request_bytes)
        .map_err(|e| NetError::new(format!("TLS write: {e}")))?;
    read_response(&mut stream)
}

fn redirect_target(
    parsed: &url::Url,
    response: &HttpResponse,
    redirects: usize,
    max_redirects: usize,
) -> Option<String> {
    if !is_redirect_status(response.status) || redirects >= max_redirects {
        return None;
    }
    response.header("location").map(|location| {
        parsed
            .join(location)
            .map_or_else(|_| location.to_string(), |u| u.to_string())
    })
}

fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

impl BasicClient {
    /*
     * fetch_parallel -- fetch multiple URLs, using HTTP/2 if all share an HTTPS host.
     *
     * WHY: chatgpt.com CSS subresources are currently fetched sequentially (one
     * TCP+TLS per request). HTTP/2 multiplexes all over one connection, saving
     * ~50ms per subresource. Three stylesheets: ~100ms savings on first render.
     *
     * Algorithm:
     *   1. If all requests are HTTPS with the same host+port: try h2_client
     *   2. On h2 success: return responses directly (all parallel over one conn)
     *   3. On h2 failure (server doesn't support h2, or any error): fall back to
     *      sequential HTTP/1.1 -- same as calling fetch() N times in a loop
     *   4. Mixed hosts or HTTP: always sequential HTTP/1.1 (no h2 benefit)
     *
     * INVARIANT: responses are returned in the same order as requests.
     *
     * Complexity: O(1) TLS handshakes on h2 path; O(N) on HTTP/1.1 fallback
     * See: h2_client.rs for the H2 implementation
     * See: SpeculativeRenderer::fetch_all_or_speculate for cache integration
     */
    #[must_use]
    pub fn fetch_parallel(&self, requests: &[HttpRequest]) -> Vec<Result<HttpResponse, NetError>> {
        if requests.is_empty() {
            return vec![];
        }

        // Try the HTTP/2 multiplexed path for same-HTTPS-host requests.
        // Every URL parses exactly once, up front. A malformed URL sends the
        // whole batch down the HTTP/1.1 path, where fetch() reports a proper
        // per-request error -- never a silently rewritten request target.
        let parsed_urls: Option<Vec<url::Url>> = requests
            .iter()
            .map(|r| url::Url::parse(&r.url).ok())
            .collect();
        if let Some(parsed_urls) = &parsed_urls
            && let Some((host, port)) = same_https_host(parsed_urls)
        {
            let h2_config = self.tls.h2_config();
            let h2_reqs: Vec<h2_client::H2Request> = requests
                .iter()
                .zip(parsed_urls)
                .map(|(r, parsed)| h2_client::H2Request {
                    path: parsed.path().to_string(),
                    query: parsed.query().map(std::string::ToString::to_string),
                    extra_headers: r.headers.clone(),
                })
                .collect();

            match h2_client::fetch_h2_parallel(h2_config, &host, port, &h2_reqs) {
                Ok(responses) => {
                    return responses
                        .into_iter()
                        .map(|r| {
                            decode_response(HttpResponse {
                                status: r.status,
                                headers: r.headers,
                                body: r.body,
                            })
                        })
                        .collect();
                }
                Err(e) => {
                    eprintln!("[SilkSurf] h2 fetch failed ({e}), falling back to HTTP/1.1");
                }
            }
        }

        // HTTP/1.1 sequential fallback (different hosts, HTTP, or h2 failure).
        requests.iter().map(|req| self.fetch(req)).collect()
    }
}

/*
 * same_https_host -- return (host, port) when every parsed URL targets the
 * same HTTPS host.
 *
 * HTTP/2 multiplexing only benefits requests to the same server over one
 * connection; mixed hosts or HTTP requests go through sequential HTTP/1.1.
 * The caller (fetch_parallel) parses the request URLs exactly once and
 * passes them here, so no URL string is ever re-parsed or substituted.
 *
 * Returns None if: urls is empty, any URL is HTTP, or hosts/ports differ.
 */
fn same_https_host(urls: &[url::Url]) -> Option<(String, u16)> {
    let first = urls.first()?;
    if first.scheme() != "https" {
        return None;
    }
    let host = first.host_str()?.to_string();
    let port = first.port().unwrap_or(443);
    for parsed in urls.iter().skip(1) {
        if parsed.scheme() != "https" {
            return None;
        }
        if parsed.host_str() != Some(host.as_str()) {
            return None;
        }
        if parsed.port().unwrap_or(443) != port {
            return None;
        }
    }
    Some((host, port))
}

#[cfg(feature = "content-encoding")]
fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}

/// Read the full HTTP response from a stream.
fn read_response(stream: &mut dyn Read) -> Result<Vec<u8>, NetError> {
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                // DoS bound (P8.S8): cap response body at MAX_RESPONSE_BODY_BYTES.
                if buf.len() > MAX_RESPONSE_BODY_BYTES {
                    return Err(NetError::new(format!(
                        "Response exceeds MAX_RESPONSE_BODY_BYTES ({MAX_RESPONSE_BODY_BYTES} bytes)"
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => return Err(NetError::new(format!("Read error: {e}"))),
        }
    }
    Ok(buf)
}

/// Parse raw HTTP response bytes into `HttpResponse` using httparse.
fn parse_response(data: &[u8]) -> Result<HttpResponse, NetError> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);

    let header_len = match response.parse(data) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) => {
            return Err(NetError::new("Incomplete HTTP response headers"));
        }
        Err(e) => return Err(NetError::new(format!("HTTP parse error: {e}"))),
    };

    let status = response.code.unwrap_or(0);
    let parsed_headers: Vec<(String, String)> = response
        .headers
        .iter()
        .filter(|h| !h.name.is_empty())
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).to_string(),
            )
        })
        .collect();

    let body = data[header_len..].to_vec();

    decode_response(HttpResponse {
        status,
        headers: parsed_headers,
        body,
    })
}

fn decode_response(response: HttpResponse) -> Result<HttpResponse, NetError> {
    let response = decode_transfer_response(response)?;
    #[cfg(feature = "content-encoding")]
    {
        decode_encoded_response(response)
    }
    #[cfg(not(feature = "content-encoding"))]
    {
        Ok(response)
    }
}

fn decode_transfer_response(mut response: HttpResponse) -> Result<HttpResponse, NetError> {
    let codings = transfer_codings(&response);
    if codings.is_empty() {
        return Ok(response);
    }
    if !transfer_codings_are_supported(&codings) {
        return Err(NetError::new(format!(
            "Unsupported Transfer-Encoding: {}",
            codings.join(", ")
        )));
    }
    response.body = decode_chunked_body(&response.body)?;
    response
        .headers
        .retain(|(name, _)| !is_decoded_transfer_header(name));
    Ok(response)
}

fn transfer_codings(response: &HttpResponse) -> Vec<String> {
    response
        .headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|coding| !coding.is_empty())
        .map(str::to_string)
        .collect()
}

fn transfer_codings_are_supported(codings: &[String]) -> bool {
    codings.len() == 1 && codings[0].eq_ignore_ascii_case("chunked")
}

fn is_decoded_transfer_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("transfer-encoding") || name.eq_ignore_ascii_case("content-length")
}

fn decode_chunked_body(body: &[u8]) -> Result<Vec<u8>, NetError> {
    let mut decoded = Vec::with_capacity(body.len());
    let mut cursor = 0;
    loop {
        let (line, after_line) = chunk_line(body, cursor)?;
        let chunk_len = chunk_len(line)?;
        cursor = after_line;
        if chunk_len == 0 {
            consume_chunk_trailers(body, cursor)?;
            return Ok(decoded);
        }
        let chunk_end = cursor
            .checked_add(chunk_len)
            .ok_or_else(|| NetError::new("Chunked response size overflow"))?;
        if chunk_end > body.len() {
            return Err(NetError::new("Incomplete chunked response body"));
        }
        let next_len = decoded.len().saturating_add(chunk_len);
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(NetError::new(format!(
                "Decoded chunked response exceeds MAX_RESPONSE_BODY_BYTES ({MAX_RESPONSE_BODY_BYTES} bytes)"
            )));
        }
        decoded.extend_from_slice(&body[cursor..chunk_end]);
        cursor = consume_chunk_crlf(body, chunk_end)?;
    }
}

fn chunk_line(body: &[u8], cursor: usize) -> Result<(&[u8], usize), NetError> {
    let Some(relative_end) = body[cursor..].windows(2).position(|pair| pair == b"\r\n") else {
        return Err(NetError::new("Incomplete chunked response size line"));
    };
    let line_end = cursor + relative_end;
    Ok((&body[cursor..line_end], line_end + 2))
}

fn chunk_len(line: &[u8]) -> Result<usize, NetError> {
    let size = line.split(|byte| *byte == b';').next().unwrap_or_default();
    let size_text = std::str::from_utf8(size)
        .map(str::trim)
        .map_err(|_| NetError::new("Invalid chunked response size"))?;
    usize::from_str_radix(size_text, 16).map_err(|_| NetError::new("Invalid chunked response size"))
}

fn consume_chunk_crlf(body: &[u8], cursor: usize) -> Result<usize, NetError> {
    if body.get(cursor..cursor + 2) == Some(b"\r\n") {
        return Ok(cursor + 2);
    }
    Err(NetError::new("Missing chunked response terminator"))
}

fn consume_chunk_trailers(body: &[u8], cursor: usize) -> Result<(), NetError> {
    if body.get(cursor..cursor + 2) == Some(b"\r\n") {
        return Ok(());
    }
    if body[cursor..]
        .windows(4)
        .any(|window| window == b"\r\n\r\n")
    {
        return Ok(());
    }
    Err(NetError::new("Incomplete chunked response trailers"))
}

#[cfg(feature = "content-encoding")]
fn decode_encoded_response(mut response: HttpResponse) -> Result<HttpResponse, NetError> {
    let Some(value) = response.header("content-encoding").map(str::to_string) else {
        return Ok(response);
    };
    let mut body = std::mem::take(&mut response.body);
    for coding in value.split(',').map(str::trim).rev() {
        if coding.is_empty() || coding.eq_ignore_ascii_case("identity") {
            continue;
        }
        body = decode_single_content_coding(coding, &body)?;
    }
    response.body = body;
    response
        .headers
        .retain(|(name, _)| !is_decoded_entity_header(name));
    Ok(response)
}

#[cfg(feature = "content-encoding")]
fn is_decoded_entity_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("content-encoding") || name.eq_ignore_ascii_case("content-length")
}

#[cfg(feature = "content-encoding")]
fn decode_single_content_coding(coding: &str, body: &[u8]) -> Result<Vec<u8>, NetError> {
    if coding.eq_ignore_ascii_case("br") {
        let reader = Cursor::new(body);
        let mut decoder = brotli_decompressor::Decompressor::new(reader, 4096);
        return read_decoded_body(&mut decoder, "brotli");
    }
    if coding.eq_ignore_ascii_case("gzip") || coding.eq_ignore_ascii_case("x-gzip") {
        let reader = Cursor::new(body);
        let mut decoder = flate2::read::GzDecoder::new(reader);
        return read_decoded_body(&mut decoder, "gzip");
    }
    if coding.eq_ignore_ascii_case("deflate") {
        let reader = Cursor::new(body);
        let mut decoder = flate2::read::DeflateDecoder::new(reader);
        return read_decoded_body(&mut decoder, "deflate");
    }
    Err(NetError::new(format!(
        "Unsupported Content-Encoding: {coding}"
    )))
}

#[cfg(feature = "content-encoding")]
fn read_decoded_body(reader: &mut dyn Read, coding: &str) -> Result<Vec<u8>, NetError> {
    let mut decoded = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    loop {
        let bytes_read = reader
            .read(&mut chunk)
            .map_err(|e| NetError::new(format!("{coding} decode: {e}")))?;
        if bytes_read == 0 {
            break;
        }
        let next_len = decoded.len().saturating_add(bytes_read);
        if next_len > MAX_RESPONSE_BODY_BYTES {
            return Err(NetError::new(format!(
                "Decoded response exceeds MAX_RESPONSE_BODY_BYTES ({MAX_RESPONSE_BODY_BYTES} bytes)"
            )));
        }
        decoded.extend_from_slice(&chunk[..bytes_read]);
    }
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::parse_response;
    #[cfg(feature = "content-encoding")]
    use super::{HttpResponse, decode_response, has_header};

    #[cfg(feature = "content-encoding")]
    use flate2::Compression;
    #[cfg(feature = "content-encoding")]
    use flate2::write::{DeflateEncoder, GzEncoder};
    #[cfg(feature = "content-encoding")]
    use std::io::Write;

    #[cfg(feature = "content-encoding")]
    #[test]
    fn has_header_matches_case_insensitively() {
        let headers = vec![("Accept-Encoding".to_string(), "identity".to_string())];
        assert!(has_header(&headers, "accept-encoding"));
        assert!(!has_header(&headers, "content-encoding"));
    }

    #[cfg(feature = "content-encoding")]
    #[test]
    fn parse_response_decodes_gzip_body() {
        let body = b"silksurf compressed html";
        let compressed = gzip_bytes(body);
        let mut raw =
            b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 999\r\n\r\n".to_vec();
        raw.extend_from_slice(&compressed);

        let response = parse_response(&raw).expect("gzip response decodes");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, body);
        assert_eq!(response.header("content-encoding"), None);
        assert_eq!(response.header("content-length"), None);
    }

    #[cfg(feature = "content-encoding")]
    #[test]
    fn parse_response_decodes_chunked_gzip_body() {
        let body = b"silksurf chunked compressed html";
        let chunked = chunked_body(&gzip_bytes(body), &[3, 5, 9]);
        let mut raw =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Encoding: gzip\r\n\r\n"
                .to_vec();
        raw.extend_from_slice(&chunked);

        let response = parse_response(&raw).expect("chunked gzip response decodes");

        assert_eq!(response.body, body);
        assert_eq!(response.header("transfer-encoding"), None);
        assert_eq!(response.header("content-encoding"), None);
        assert_eq!(response.header("content-length"), None);
    }

    #[test]
    fn parse_response_rejects_incomplete_chunked_body() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nsil".to_vec();

        let err = parse_response(&raw).expect_err("incomplete chunk fails");

        assert!(err.message.contains("Incomplete chunked response body"));
    }

    #[cfg(feature = "content-encoding")]
    #[test]
    fn decode_response_decodes_deflate_body() {
        let body = b"css payload";
        let response = HttpResponse {
            status: 200,
            headers: vec![("Content-Encoding".to_string(), "deflate".to_string())],
            body: deflate_bytes(body),
        };

        let decoded = decode_response(response).expect("deflate response decodes");

        assert_eq!(decoded.body, body);
        assert_eq!(decoded.header("content-encoding"), None);
    }

    #[cfg(feature = "content-encoding")]
    #[test]
    fn decode_response_rejects_unknown_content_coding() {
        let response = HttpResponse {
            status: 200,
            headers: vec![("Content-Encoding".to_string(), "zstd".to_string())],
            body: b"payload".to_vec(),
        };

        let err = decode_response(response).expect_err("unknown coding fails");

        assert!(err.message.contains("Unsupported Content-Encoding"));
    }

    #[cfg(feature = "content-encoding")]
    fn gzip_bytes(body: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(body).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    #[cfg(feature = "content-encoding")]
    fn deflate_bytes(body: &[u8]) -> Vec<u8> {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(body).expect("deflate write");
        encoder.finish().expect("deflate finish")
    }

    #[cfg(feature = "content-encoding")]
    fn chunked_body(body: &[u8], split_hint: &[usize]) -> Vec<u8> {
        let mut chunked = Vec::new();
        let mut cursor = 0;
        for &hint in split_hint {
            if cursor >= body.len() {
                break;
            }
            let end = cursor.saturating_add(hint).min(body.len());
            write!(chunked, "{:x}\r\n", end - cursor).expect("chunk size write");
            chunked.extend_from_slice(&body[cursor..end]);
            chunked.extend_from_slice(b"\r\n");
            cursor = end;
        }
        if cursor < body.len() {
            write!(chunked, "{:x}\r\n", body.len() - cursor).expect("chunk size write");
            chunked.extend_from_slice(&body[cursor..]);
            chunked.extend_from_slice(b"\r\n");
        }
        chunked.extend_from_slice(b"0\r\n\r\n");
        chunked
    }
}
