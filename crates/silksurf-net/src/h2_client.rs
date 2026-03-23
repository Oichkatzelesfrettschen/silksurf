/*
 * h2_client.rs -- HTTP/2 parallel fetch over a single TLS connection.
 *
 * WHY: chatgpt.com serves 2-3 CSS files as sequential HTTP/1.1 requests, each
 * incurring TCP+TLS setup (~50ms) + transfer time. HTTP/2 multiplexes all
 * requests over a single TLS connection, so total time = max(RTT) not sum(RTT).
 * Typical saving: (N-1) * ~50ms = ~100-150ms for 3 subresources.
 *
 * Architecture:
 *   fetch_h2_parallel(config, host, port, requests) -- blocking entry point
 *     -> creates tokio current_thread runtime (zero OS threads, drives I/O on caller)
 *     -> fetch_h2_parallel_async -- async h2 client via tokio-rustls
 *       1. TCP connect (tokio)
 *       2. TLS handshake (tokio-rustls) -- config has ALPN ["h2", "http/1.1"]
 *       3. Check negotiated ALPN -- return Err("not h2") if server chose HTTP/1.1
 *       4. h2::client::handshake -- negotiate HTTP/2 settings
 *       5. Send all requests in a loop (all in-flight before any await)
 *       6. Collect responses in send-order
 *
 * Thread model: all-sync from the caller's perspective. The tokio runtime is
 * created and dropped within fetch_h2_parallel; it never spawns OS threads.
 *
 * Complexity: O(1) TLS handshakes (vs O(N) for HTTP/1.1), O(N) request frames
 * Fallback: caller checks Err("not h2") and falls back to sequential HTTP/1.1
 *
 * See: BasicClient::fetch_parallel in lib.rs for integration point
 * See: TlsConfig::new_h2 in silksurf-tls for ALPN configuration
 */

use bytes::Bytes;
use h2::client;
use http::{Method, Request, Version};
use rustls::ClientConfig;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

/*
 * H2Request -- minimal per-URL state for parallel h2 fetch.
 *
 * path and query are separated because h2 sends them in the :path pseudo-header.
 * headers are request-specific (e.g. Accept: text/css).
 */
pub struct H2Request {
    pub path: String,
    pub query: Option<String>,
    pub extra_headers: Vec<(String, String)>,
}

/*
 * H2Response -- response from a single h2 stream.
 *
 * Mirrors HttpResponse but avoids re-importing silksurf-net types here.
 */
pub struct H2Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/*
 * fetch_h2_parallel -- blocking entry point for HTTP/2 parallel fetch.
 *
 * Creates a minimal tokio current_thread runtime (zero extra OS threads) to
 * drive the h2 state machine. The runtime is dropped when this function returns.
 *
 * Returns Err if the server did not negotiate h2 via ALPN, or if any network
 * error occurred. Callers should fall back to sequential HTTP/1.1 on Err.
 *
 * INVARIANT: tls_config must have ALPN ["h2", "http/1.1"]; use TlsProvider::h2_config().
 * A config without ALPN will negotiate HTTP/1.1 and this function will return Err.
 *
 * Complexity: O(1) TLS handshakes + O(N) request frames per call
 */
pub fn fetch_h2_parallel(
    tls_config: Arc<ClientConfig>,
    host: &str,
    port: u16,
    requests: &[H2Request],
) -> Result<Vec<H2Response>, String> {
    if requests.is_empty() {
        return Ok(vec![]);
    }
    /*
     * new_current_thread: runs on the calling OS thread -- no extra threads created.
     * enable_io: needed for TcpStream async I/O (epoll/io_uring on Linux)
     * enable_time: needed by h2's internal keepalive timers
     */
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|e| format!("tokio runtime: {e}"))?;

    rt.block_on(fetch_h2_parallel_async(tls_config, host, port, requests))
}

async fn fetch_h2_parallel_async(
    tls_config: Arc<ClientConfig>,
    host: &str,
    port: u16,
    requests: &[H2Request],
) -> Result<Vec<H2Response>, String> {
    let addr = format!("{host}:{port}");
    let tcp = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("TCP connect {addr}: {e}"))?;
    tcp.set_nodelay(true).ok();

    /*
     * TLS handshake via tokio-rustls. The config has ALPN ["h2", "http/1.1"],
     * so the server will negotiate h2 if it supports it.
     */
    let server_name = rustls::pki_types::ServerName::try_from(host)
        .map_err(|e| format!("server name: {e}"))?
        .to_owned();
    let connector = TlsConnector::from(tls_config);
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| format!("TLS handshake: {e}"))?;

    /*
     * ALPN check: if the server negotiated http/1.1 (or no ALPN), we cannot
     * use h2 framing. Return Err so the caller can fall back to HTTP/1.1.
     */
    let negotiated = tls.get_ref().1.alpn_protocol();
    if negotiated != Some(b"h2") {
        let proto = negotiated
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .unwrap_or_else(|| "none".to_string());
        return Err(format!("server negotiated '{proto}' not 'h2'"));
    }

    /*
     * h2 handshake: sends HTTP/2 client preface + initial SETTINGS frame.
     * The connection future must be driven concurrently with request sending;
     * we spawn it on the current_thread runtime (same OS thread, cooperative).
     */
    let (mut send_request, connection) = client::handshake(tls)
        .await
        .map_err(|e| format!("h2 handshake: {e}"))?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("[h2] connection error: {e}");
        }
    });

    /*
     * Send all requests before awaiting any response.
     *
     * WHY: h2 multiplexing means all requests are in-flight simultaneously.
     * Awaiting each response before sending the next would serialize the
     * round-trips, defeating multiplexing. Instead: send all -> collect all.
     *
     * ready().await resolves immediately when under SETTINGS_MAX_CONCURRENT_STREAMS
     * (typically 100 on modern servers). For 2-3 CSS files, always immediate.
     */
    let mut response_futures = Vec::with_capacity(requests.len());
    for req in requests {
        let uri = match &req.query {
            Some(q) => format!("https://{host}{}?{q}", req.path),
            None => format!("https://{host}{}", req.path),
        };

        let mut builder = Request::builder()
            .method(Method::GET)
            .uri(&uri)
            .version(Version::HTTP_2)
            .header("accept", "text/css,*/*")
            .header("user-agent", "SilkSurf/0.1 (X11; Linux x86_64)");
        for (k, v) in &req.extra_headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let http_req = builder
            .body(())
            .map_err(|e| format!("build request: {e}"))?;

        /*
         * ready() consumes send_request and resolves to it again when the
         * connection has capacity (SETTINGS_MAX_CONCURRENT_STREAMS not exceeded).
         * Reassign so the loop variable is valid for the next iteration.
         */
        send_request = send_request
            .ready()
            .await
            .map_err(|e| format!("h2 ready: {e}"))?;
        let (resp_future, _send_stream) = send_request
            .send_request(http_req, true)
            .map_err(|e| format!("send request: {e}"))?;

        response_futures.push(resp_future);
    }

    /*
     * Collect responses in send order.
     *
     * All responses are in-flight (server is processing them in parallel).
     * Awaiting them sequentially is fine: the server processes them concurrently,
     * so total time = max(individual RTTs), not their sum.
     *
     * Flow control: after reading each DATA frame chunk, release capacity back
     * to the connection window. Without this, the server will stall after the
     * first window's worth of data (~65KB default initial window).
     */
    let mut results = Vec::with_capacity(requests.len());
    for resp_future in response_futures {
        let response = resp_future
            .await
            .map_err(|e| format!("response: {e}"))?;

        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| {
                (
                    k.as_str().to_string(),
                    v.to_str().unwrap_or("").to_string(),
                )
            })
            .collect();

        let mut body_stream = response.into_body();
        let mut body: Vec<u8> = Vec::new();
        while let Some(chunk) = body_stream.data().await {
            let data: Bytes = chunk.map_err(|e| format!("body data: {e}"))?;
            let n = data.len();
            body.extend_from_slice(&data);
            body_stream.flow_control().release_capacity(n).ok();
        }

        results.push(H2Response {
            status,
            headers,
            body,
        });
    }

    Ok(results)
}
