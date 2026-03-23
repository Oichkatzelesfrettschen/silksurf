//! Networking, fetch pipeline, and caching (cleanroom).
#![allow(clippy::collapsible_if)]
//!
//! Pure Rust HTTP/1.1 client using:
//! - rustls for TLS (no OpenSSL)
//! - httparse for zero-copy header parsing
//! - url for WHATWG URL parsing
//! - std::net::TcpStream for synchronous TCP (no async runtime)

use rustls::StreamOwned;
use silksurf_tls::{RustlsProvider, TlsProvider};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

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
            let tcp = TcpStream::connect(&addr)
                .map_err(|e| NetError::new(format!("TCP connect to {addr}: {e}")))?;
            tcp.set_read_timeout(Some(std::time::Duration::from_secs(30)))
                .ok();

            // Send request and read response
            let response_bytes = if is_https {
                let server_name = rustls::pki_types::ServerName::try_from(host.as_str())
                    .map_err(|e| NetError::new(format!("Invalid server name: {e}")))?
                    .to_owned();
                let config = self.tls.config();
                let conn = rustls::ClientConnection::new(config, server_name)
                    .map_err(|e| NetError::new(format!("TLS handshake: {e}")))?;
                let mut stream = StreamOwned::new(conn, tcp);
                stream
                    .write_all(&req_buf)
                    .map_err(|e| NetError::new(format!("TLS write: {e}")))?;
                read_response(&mut stream)?
            } else {
                let mut tcp = tcp;
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

/// Read the full HTTP response from a stream.
fn read_response(stream: &mut dyn Read) -> Result<Vec<u8>, NetError> {
    let mut buf = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                // Safety limit: 16MB
                if buf.len() > 16 * 1024 * 1024 {
                    return Err(NetError::new("Response exceeds 16MB limit"));
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
