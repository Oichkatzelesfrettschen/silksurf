//! TLS adapter layer for SilkSurf (cleanroom).

use rustls::{ClientConfig, RootCertStore};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TlsConfig {
    inner: Arc<ClientConfig>,
}

impl TlsConfig {
    pub fn new() -> Self {
        let roots = RootCertStore::empty();
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self {
            inner: Arc::new(config),
        }
    }

    pub fn inner(&self) -> Arc<ClientConfig> {
        self.inner.clone()
    }
}

#[derive(Debug, Clone)]
pub struct RustlsProvider {
    config: TlsConfig,
}

impl RustlsProvider {
    pub fn new() -> Self {
        Self {
            config: TlsConfig::new(),
        }
    }
}

pub trait TlsProvider {
    fn config(&self) -> Arc<ClientConfig>;
}

impl TlsProvider for RustlsProvider {
    fn config(&self) -> Arc<ClientConfig> {
        self.config.inner()
    }
}
