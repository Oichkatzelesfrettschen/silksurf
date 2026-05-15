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
pub mod h2_client;

use rustls::StreamOwned;
use silksurf_tls::{RustlsProvider, TlsProvider};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

impl HttpMethod {
    fn as_str(&self) -> &'static str {
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

pub struct BasicClient {
    tls: Arc<dyn TlsProvider + Send + Sync>,
    max_redirects: usize,
}

impl BasicClient {
    pub fn new() -> Self {
        Self {
            tls: Arc::new(RustlsProvider::new()),
            max_redirects: 5,
        }
    }

    pub fn with_tls(tls: Arc<dyn TlsProvider + Send + Sync>) -> Self {
        Self {
            tls,
            max_redirects: 5,
        }
    }
}

impl Default for BasicClient {
    fn default() -> Self {
        Self::new()
    }
}

impl NetClient for BasicClient {
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        let mut current_url = request.url.clone();
        let mut redirects = 0;

        loop {
            let parsed = url::Url::parse(&current_url)
                .map_err(|e| NetError::new(format!("Invalid URL: {e}")))?;

            let host = parsed
                .host_str()
                .ok_or_else(|| NetError::new("No host in URL"))?
                .to_string();
            let is_https = parsed.scheme() == "https";
            let port = parsed.port().unwrap_or(if is_https { 443 } else { 80 });
            let path = if parsed.query().is_some() {
                format!("{}?{}", parsed.path(), parsed.query().unwrap_or(""))
            } else {
                parsed.path().to_string()
            };

            // Build HTTP/1.1 request
            let mut req_buf = Vec::with_capacity(512);
            write!(
                req_buf,
                "{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: SilkSurf/0.1\r\nAccept: */*\r\n",
                request.method.as_str(),
                if path.is_empty() { "/" } else { &path },
            ).map_err(|e| NetError::new(format!("Write error: {e}")))?;

            // Add custom headers
            for (name, value) in &request.headers {
                write!(req_buf, "{name}: {value}\r\n")
                    .map_err(|e| NetError::new(format!("Write error: {e}")))?;
            }

            // Content-Length for POST bodies
            if !request.body.is_empty() {
                write!(req_buf, "Content-Length: {}\r\n", request.body.len())
                    .map_err(|e| NetError::new(format!("Write error: {e}")))?;
            }

            req_buf.extend_from_slice(b"\r\n");
            req_buf.extend_from_slice(&request.body);

            // Connect
            let addr = format!("{host}:{port}");
            let mut tcp = TcpStream::connect(&addr)
                .map_err(|e| NetError::new(format!("TCP connect to {addr}: {e}")))?;
            // DoS bound (P8.S8): cap stalls during handshake/read. Aligned
            // with silksurf_tls::MAX_TLS_HANDSHAKE_SECS so handshake
            // exhaustion attacks become recoverable NetError.
            tcp.set_read_timeout(Some(std::time::Duration::from_secs(
                silksurf_tls::MAX_TLS_HANDSHAKE_SECS,
            )))
            .ok();

            // Send request and read response
            let response_bytes = if is_https {
                let server_name = rustls::pki_types::ServerName::try_from(host.as_str())
                    .map_err(|e| NetError::new(format!("Invalid server name: {e}")))?
                    .to_owned();
                let config = self.tls.config();
                let mut conn = rustls::ClientConnection::new(config, server_name)
                    .map_err(|e| NetError::new(format!("TLS setup: {e}")))?;
                while conn.is_handshaking() {
                    conn.complete_io(&mut tcp)
                        .map_err(|e| NetError::new(format!("TLS handshake: {e}")))?;
                }
                let mut stream = StreamOwned::new(conn, tcp);
                stream
                    .write_all(&req_buf)
                    .map_err(|e| NetError::new(format!("TLS write: {e}")))?;
                read_response(&mut stream)?
            } else {
                tcp.write_all(&req_buf)
                    .map_err(|e| NetError::new(format!("TCP write: {e}")))?;
                read_response(&mut tcp)?
            };

            // Parse response
            let response = parse_response(&response_bytes)?;

            // Handle redirects
            if matches!(response.status, 301 | 302 | 303 | 307 | 308)
                && redirects < self.max_redirects
            {
                if let Some(location) = response.header("location") {
                    current_url = parsed
                        .join(location)
                        .map(|u| u.to_string())
                        .unwrap_or_else(|_| location.to_string());
                    redirects += 1;
                    continue;
                }
            }

            return Ok(response);
        }
    }
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
    pub fn fetch_parallel(&self, requests: &[HttpRequest]) -> Vec<Result<HttpResponse, NetError>> {
        if requests.is_empty() {
            return vec![];
        }

        // Try the HTTP/2 multiplexed path for same-HTTPS-host requests.
        if let Some((host, port)) = same_https_host(requests) {
            let h2_config = self.tls.h2_config();
            let h2_reqs: Vec<h2_client::H2Request> = requests
                .iter()
                .map(|r| {
                    // UNWRAP-OK: "https://localhost/" is a static, syntactically valid URL.
                    let parsed = url::Url::parse(&r.url)
                        .unwrap_or_else(|_| url::Url::parse("https://localhost/").unwrap());
                    h2_client::H2Request {
                        path: parsed.path().to_string(),
                        query: parsed.query().map(|q| q.to_string()),
                        extra_headers: r.headers.clone(),
                    }
                })
                .collect();

            match h2_client::fetch_h2_parallel(h2_config, &host, port, &h2_reqs) {
                Ok(responses) => {
                    return responses
                        .into_iter()
                        .map(|r| {
                            Ok(HttpResponse {
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
 * same_https_host -- return (host, port) if all requests target the same HTTPS host.
 *
 * WHY: HTTP/2 multiplexing only benefits requests to the same server over one
 * connection. Mixed hosts or HTTP requests go through sequential HTTP/1.1.
 *
 * Returns None if: requests is empty, any URL is HTTP, or hosts differ.
 */
fn same_https_host(requests: &[HttpRequest]) -> Option<(String, u16)> {
    if requests.is_empty() {
        return None;
    }
    let first = url::Url::parse(&requests[0].url).ok()?;
    if first.scheme() != "https" {
        return None;
    }
    let host = first.host_str()?.to_string();
    let port = first.port().unwrap_or(443);
    for req in requests.iter().skip(1) {
        let parsed = url::Url::parse(&req.url).ok()?;
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
                        "Response exceeds MAX_RESPONSE_BODY_BYTES ({} bytes)",
                        MAX_RESPONSE_BODY_BYTES
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

/// Parse raw HTTP response bytes into HttpResponse using httparse.
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

    Ok(HttpResponse {
        status,
        headers: parsed_headers,
        body,
    })
}
