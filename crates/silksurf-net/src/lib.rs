//! Networking, fetch pipeline, and caching (cleanroom).

use silksurf_tls::{RustlsProvider, TlsProvider};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetError {
    pub message: String,
}

pub trait NetClient {
    fn fetch(&self, request: &HttpRequest) -> Result<HttpResponse, NetError>;
}

pub struct BasicClient {
    tls: Arc<dyn TlsProvider + Send + Sync>,
}

impl BasicClient {
    pub fn new() -> Self {
        Self {
            tls: Arc::new(RustlsProvider::new()),
        }
    }

    pub fn with_tls(tls: Arc<dyn TlsProvider + Send + Sync>) -> Self {
        Self { tls }
    }

    pub fn tls(&self) -> Arc<dyn TlsProvider + Send + Sync> {
        self.tls.clone()
    }
}

impl NetClient for BasicClient {
    fn fetch(&self, _request: &HttpRequest) -> Result<HttpResponse, NetError> {
        Err(NetError {
            message: "fetch not implemented".to_string(),
        })
    }
}
