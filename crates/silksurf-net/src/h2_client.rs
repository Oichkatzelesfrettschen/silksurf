/*
 * h2_client.rs drives same-origin HTTPS batches over one HTTP/2 TLS
 * connection.
 *
 * fetch_h2_parallel creates a current-thread Tokio runtime, negotiates ALPN,
 * sends every request before awaiting bodies, and returns responses in request
 * order. BasicClient::fetch_parallel owns the HTTP/1.1 fallback boundary.
 */

use bytes::Bytes;
use futures_util::future::join_all;
use h2::client::{self, ResponseFuture, SendRequest};
use http::{Method, Request, Version};
use rustls::ClientConfig;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;

#[cfg(feature = "content-encoding")]
const ACCEPT_ENCODING_VALUE: &str = "br, gzip, deflate";

/*
 * H2Request stores the per-stream :path components and request-specific
 * headers used by the HTTP/2 request builder.
 */
pub struct H2Request {
    pub path: String,
    pub query: Option<String>,
    pub extra_headers: Vec<(String, String)>,
}

/*
 * H2Response mirrors HttpResponse while this module stays independent of the
 * public client response type.
 */
pub struct H2Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/*
 * fetch_h2_parallel presents a blocking API while the current-thread runtime
 * drives TCP, TLS, and HTTP/2 state on the caller thread.
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
     * new_current_thread keeps the network batch on the caller OS thread.
     * enable_io drives TcpStream, and enable_time drives h2 timers.
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
     * tokio-rustls uses the caller-provided ALPN config, so h2-capable servers
     * select HTTP/2 during the TLS handshake.
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
     * A non-h2 ALPN result cannot carry HTTP/2 frames. The caller owns the
     * HTTP/1.1 fallback path.
     */
    let negotiated = tls.get_ref().1.alpn_protocol();
    if negotiated != Some(b"h2") {
        let proto = negotiated.map_or_else(
            || "none".to_string(),
            |p| String::from_utf8_lossy(p).into_owned(),
        );
        return Err(format!("server negotiated '{proto}' not 'h2'"));
    }

    /*
     * h2::client::handshake sends the client preface and SETTINGS frame. The
     * connection future runs cooperatively on the same current-thread runtime.
     */
    let (send_request, connection) = client::handshake(tls)
        .await
        .map_err(|e| format!("h2 handshake: {e}"))?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("[h2] connection error: {e}");
        }
    });

    /*
     * The sender enqueues every request before the collector awaits bodies.
     * That keeps same-origin resource batches multiplexed instead of serialized.
     */
    let response_futures = send_h2_requests(send_request, host, requests).await?;

    /*
     * H2 response bodies drain concurrently. Each DATA frame returns flow-control
     * capacity while other streams remain in flight, so large module batches do
     * not starve the connection window or trigger peer resets.
     */
    collect_h2_responses(response_futures).await
}

async fn send_h2_requests(
    mut send_request: SendRequest<Bytes>,
    host: &str,
    requests: &[H2Request],
) -> Result<Vec<ResponseFuture>, String> {
    let mut response_futures = Vec::with_capacity(requests.len());
    for req in requests {
        let http_req = build_h2_request(host, req)?;
        send_request = send_request
            .ready()
            .await
            .map_err(|e| format!("h2 ready: {e}"))?;
        let (resp_future, _send_stream) = send_request
            .send_request(http_req, true)
            .map_err(|e| format!("send request: {e}"))?;
        response_futures.push(resp_future);
    }
    Ok(response_futures)
}

fn build_h2_request(host: &str, req: &H2Request) -> Result<Request<()>, String> {
    let uri = match &req.query {
        Some(query) => format!("https://{host}{}?{query}", req.path),
        None => format!("https://{host}{}", req.path),
    };
    let mut builder = Request::builder()
        .method(Method::GET)
        .uri(&uri)
        .version(Version::HTTP_2)
        .header("accept", "text/css,*/*")
        .header("user-agent", "SilkSurf/0.1 (X11; Linux x86_64)");
    #[cfg(feature = "content-encoding")]
    if !has_header(&req.extra_headers, "accept-encoding") {
        builder = builder.header("accept-encoding", ACCEPT_ENCODING_VALUE);
    }
    for (key, value) in &req.extra_headers {
        builder = builder.header(key.as_str(), value.as_str());
    }
    builder.body(()).map_err(|e| format!("build request: {e}"))
}

async fn collect_h2_responses(
    response_futures: Vec<ResponseFuture>,
) -> Result<Vec<H2Response>, String> {
    let responses = join_all(response_futures.into_iter().map(collect_h2_response)).await;
    let mut collected = Vec::with_capacity(responses.len());
    for response in responses {
        collected.push(response?);
    }
    Ok(collected)
}

async fn collect_h2_response(resp_future: ResponseFuture) -> Result<H2Response, String> {
    let response = resp_future.await.map_err(|e| format!("response: {e}"))?;
    let status = response.status().as_u16();
    let headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(key, value)| {
            (
                key.as_str().to_string(),
                value.to_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let mut body_stream = response.into_body();
    let mut body = Vec::new();
    while let Some(chunk) = body_stream.data().await {
        let data = chunk.map_err(|e| format!("body data: {e}"))?;
        let bytes_read = data.len();
        body.extend_from_slice(&data);
        body_stream.flow_control().release_capacity(bytes_read).ok();
    }
    Ok(H2Response {
        status,
        headers,
        body,
    })
}

#[cfg(feature = "content-encoding")]
fn has_header(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
}
