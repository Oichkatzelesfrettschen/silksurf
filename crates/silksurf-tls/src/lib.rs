//! TLS adapter layer for SilkSurf (cleanroom).
//!
//! Loads Mozilla root certificates (webpki-roots) plus system certificates
//! (rustls-native-certs). Provides a configured rustls ClientConfig.

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::{ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TlsConfig {
    inner: Arc<ClientConfig>,
}

impl TlsConfig {
    /// Create TLS config with Mozilla + system root certificates.
    pub fn new() -> Self {
        let mut roots = RootCertStore::empty();

        // Mozilla's root certificates (reliable baseline)
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

        // System root certificates (enterprise CAs, local trust)
        let result = rustls_native_certs::load_native_certs();
        for cert in result.certs {
            let _ = roots.add(cert);
        }

        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self {
            inner: Arc::new(config),
        }
    }

    /// Create TLS config that accepts any certificate (INSECURE -- for debugging only).
    pub fn new_insecure() -> Self {
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerifier))
            .with_no_client_auth();
        Self {
            inner: Arc::new(config),
        }
    }

    pub fn inner(&self) -> Arc<ClientConfig> {
        self.inner.clone()
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new()
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

    /// Create a provider that skips certificate verification (INSECURE).
    pub fn new_insecure() -> Self {
        Self {
            config: TlsConfig::new_insecure(),
        }
    }
}

impl Default for RustlsProvider {
    fn default() -> Self {
        Self::new()
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

/// Certificate verifier that accepts everything (DANGEROUS -- debug only).
#[derive(Debug)]
struct NoVerifier;

impl ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}
